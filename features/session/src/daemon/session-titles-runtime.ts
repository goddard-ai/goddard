import type { DaemonConfigProvider } from "@goddard-ai/daemon-plugin"
import type * as acp from "acp-client/protocol"
import { getErrorMessage } from "radashi"

import type { SessionDb } from "../daemon.ts"
import type { DaemonSession, SessionTitlesConfig } from "../schema.ts"
import type { SessionMemory } from "./session-memory.ts"
import { loadDaemonTextModel } from "./text-model-resolver.ts"
import { generateSessionTitle, prepareSessionTitle } from "./title.ts"

type SessionId = DaemonSession["id"]
type SessionTitleGeneratorConfig = NonNullable<SessionTitlesConfig["generator"]>

/** Owns asynchronous session-title preparation and generation tasks for live prompt flow. */
export function createSessionTitleRuntime({
  db,
  memory,
  configProvider,
  debug,
  emitDiagnostic,
  updateSession,
}: {
  db: SessionDb
  memory: SessionMemory
  configProvider: DaemonConfigProvider<{
    sessionTitles?: SessionTitlesConfig
  }>
  debug: (event: string, fields?: Record<string, unknown>) => void
  emitDiagnostic: (sessionId: SessionId, type: string, detail?: Record<string, unknown>) => void
  updateSession: (
    id: SessionId,
    update: Partial<DaemonSession>,
    detail?: Record<string, unknown>,
  ) => void
}) {
  const pendingPreparations = memory.pendingSessionTitlePreparations
  const pendingGenerations = memory.pendingSessionTitleGenerations

  /** Starts one detached title-generation task for a session whose fallback title is already persisted. */
  function queueSessionTitleGeneration(params: {
    id: SessionId
    generatorConfig: SessionTitleGeneratorConfig
    fallbackTitle: string
    promptText: string
  }) {
    if (pendingGenerations.has(params.id)) {
      debug("session.titles.generation_skipped", {
        sessionId: params.id,
        reason: "already_pending",
      })
      return
    }

    const task = (async () => {
      const sessionRecord = db.sessions.get(params.id) ?? null
      if (!sessionRecord || sessionRecord.titleState !== "pending") {
        debug("session.titles.generation_skipped", {
          sessionId: params.id,
          reason: sessionRecord ? "not_pending" : "missing_session",
          titleState: sessionRecord?.titleState,
        })
        return
      }

      debug("session.titles.generation_started", {
        sessionId: params.id,
        provider: params.generatorConfig.provider,
        model: params.generatorConfig.model,
        fallbackTitle: params.fallbackTitle,
      })

      try {
        const loadedTextModel = await loadDaemonTextModel(params.generatorConfig)
        const generatedTitle = await generateSessionTitle({
          model: loadedTextModel.model,
          promptText: params.promptText,
        })
        if (!generatedTitle) {
          throw new Error("Generated session title was empty or invalid.")
        }

        updateSession(params.id, {
          title: generatedTitle,
          titleState: "generated",
        })
        debug("session.titles.generated", {
          sessionId: params.id,
          provider: loadedTextModel.descriptor.provider,
          model: loadedTextModel.descriptor.model,
          title: generatedTitle,
        })
        emitDiagnostic(params.id, "session_title_generated", {
          provider: loadedTextModel.descriptor.provider,
          model: loadedTextModel.descriptor.model,
          title: generatedTitle,
        })
      } catch (error) {
        updateSession(params.id, {
          title: params.fallbackTitle,
          titleState: "failed",
        })
        debug("session.titles.generation_failed", {
          sessionId: params.id,
          provider: params.generatorConfig.provider,
          model: params.generatorConfig.model,
          errorMessage: getErrorMessage(error),
        })
        emitDiagnostic(params.id, "session_title_generation_failed", {
          provider: params.generatorConfig.provider,
          model: params.generatorConfig.model,
          errorMessage: getErrorMessage(error),
        })
      }
    })().finally(() => {
      pendingGenerations.delete(params.id)
    })

    pendingGenerations.set(params.id, task)
  }

  /** Initializes the first prompt-derived title for placeholder sessions without blocking prompt flow. */
  function queueSessionTitlePreparation(params: {
    id: SessionId
    prompt: string | acp.ContentBlock[]
  }) {
    const sessionRecord = db.sessions.get(params.id) ?? null
    if (
      !sessionRecord ||
      sessionRecord.titleState !== "placeholder" ||
      pendingPreparations.has(params.id)
    ) {
      debug("session.titles.preparation_skipped", {
        sessionId: params.id,
        reason: !sessionRecord
          ? "missing_session"
          : pendingPreparations.has(params.id)
            ? "already_pending"
            : "not_placeholder",
        titleState: sessionRecord?.titleState,
      })
      return
    }

    const task = (async () => {
      debug("session.titles.preparation_started", {
        sessionId: params.id,
        cwd: sessionRecord.cwd,
      })
      let generatorConfig = configProvider.getLastKnownRootConfig(sessionRecord.cwd)?.config
        .sessionTitles?.generator

      if (!generatorConfig) {
        try {
          generatorConfig = (await configProvider.getRootConfig(sessionRecord.cwd)).config
            .sessionTitles?.generator
        } catch {}
      }

      const preparedTitle = prepareSessionTitle(params.prompt, generatorConfig)
      if (preparedTitle.titleState === "placeholder" || !preparedTitle.promptText) {
        debug("session.titles.preparation_skipped", {
          sessionId: params.id,
          reason: "placeholder_result",
          titleState: preparedTitle.titleState,
        })
        return
      }

      updateSession(params.id, {
        title: preparedTitle.title,
        titleState: preparedTitle.titleState,
      })
      debug("session.titles.prepared", {
        sessionId: params.id,
        title: preparedTitle.title,
        titleState: preparedTitle.titleState,
        hasGenerator: Boolean(preparedTitle.generatorConfig),
      })

      if (preparedTitle.titleState === "pending" && preparedTitle.generatorConfig) {
        queueSessionTitleGeneration({
          id: params.id,
          generatorConfig: preparedTitle.generatorConfig,
          fallbackTitle: preparedTitle.title,
          promptText: preparedTitle.promptText,
        })
      }
    })()
      .catch((error) => {
        emitDiagnostic(params.id, "session_title_generation_failed", {
          errorMessage: getErrorMessage(error),
        })
      })
      .finally(() => {
        pendingPreparations.delete(params.id)
      })

    pendingPreparations.set(params.id, task)
  }

  return {
    queueSessionTitleGeneration,
    queueSessionTitlePreparation,
  }
}
