import type { DaemonConfigProvider, DaemonLogger } from "@goddard-ai/daemon-plugin"
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
export function createSessionTitleRuntime(input: {
  db: SessionDb
  memory: SessionMemory
  configProvider: DaemonConfigProvider<{
    sessionTitles?: SessionTitlesConfig
  }>
  debug: (event: string, fields?: Record<string, unknown>) => void
  emitDiagnostic: (
    sessionId: SessionId,
    type: string,
    detail?: Record<string, unknown>,
    diagnosticLogger?: DaemonLogger,
  ) => void
  updateSession: (
    id: SessionId,
    update: Partial<DaemonSession>,
    detail?: Record<string, unknown>,
    diagnosticLogger?: DaemonLogger,
  ) => void
}) {
  const pendingPreparations = input.memory.pendingSessionTitlePreparations
  const pendingGenerations = input.memory.pendingSessionTitleGenerations

  /** Starts one detached title-generation task for a session whose fallback title is already persisted. */
  function queueSessionTitleGeneration(params: {
    id: SessionId
    generatorConfig: SessionTitleGeneratorConfig
    fallbackTitle: string
    promptText: string
    diagnosticLogger?: DaemonLogger
  }) {
    if (pendingGenerations.has(params.id)) {
      input.debug("session.titles.generation_skipped", {
        sessionId: params.id,
        reason: "already_pending",
      })
      return
    }

    const task = (async () => {
      const sessionRecord = input.db.sessions.get(params.id) ?? null
      if (!sessionRecord || sessionRecord.titleState !== "pending") {
        input.debug("session.titles.generation_skipped", {
          sessionId: params.id,
          reason: sessionRecord ? "not_pending" : "missing_session",
          titleState: sessionRecord?.titleState,
        })
        return
      }

      input.debug("session.titles.generation_started", {
        sessionId: params.id,
        provider: params.generatorConfig.provider,
        model: params.generatorConfig.model,
        fallbackTitle: params.fallbackTitle,
      })
      input.emitDiagnostic(
        params.id,
        "session_title_generation_started",
        {
          provider: params.generatorConfig.provider,
          model: params.generatorConfig.model,
        },
        params.diagnosticLogger,
      )

      try {
        const loadedTextModel = await loadDaemonTextModel(params.generatorConfig)
        const generatedTitle = await generateSessionTitle({
          model: loadedTextModel.model,
          promptText: params.promptText,
        })
        if (!generatedTitle) {
          throw new Error("Generated session title was empty or invalid.")
        }

        input.updateSession(
          params.id,
          {
            title: generatedTitle,
            titleState: "generated",
          },
          undefined,
          params.diagnosticLogger,
        )
        input.debug("session.titles.generated", {
          sessionId: params.id,
          provider: loadedTextModel.descriptor.provider,
          model: loadedTextModel.descriptor.model,
          title: generatedTitle,
        })
        input.emitDiagnostic(
          params.id,
          "session_title_generated",
          {
            provider: loadedTextModel.descriptor.provider,
            model: loadedTextModel.descriptor.model,
            title: generatedTitle,
          },
          params.diagnosticLogger,
        )
      } catch (error) {
        input.updateSession(
          params.id,
          {
            title: params.fallbackTitle,
            titleState: "failed",
          },
          undefined,
          params.diagnosticLogger,
        )
        input.debug("session.titles.generation_failed", {
          sessionId: params.id,
          provider: params.generatorConfig.provider,
          model: params.generatorConfig.model,
          errorMessage: getErrorMessage(error),
        })
        input.emitDiagnostic(
          params.id,
          "session_title_generation_failed",
          {
            provider: params.generatorConfig.provider,
            model: params.generatorConfig.model,
            errorMessage: getErrorMessage(error),
          },
          params.diagnosticLogger,
        )
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
    diagnosticLogger?: DaemonLogger
  }) {
    const sessionRecord = input.db.sessions.get(params.id) ?? null
    if (
      !sessionRecord ||
      sessionRecord.titleState !== "placeholder" ||
      pendingPreparations.has(params.id)
    ) {
      input.debug("session.titles.preparation_skipped", {
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
      input.debug("session.titles.preparation_started", {
        sessionId: params.id,
        cwd: sessionRecord.cwd,
      })
      let generatorConfig = input.configProvider.getLastKnownRootConfig(sessionRecord.cwd)?.config
        .sessionTitles?.generator

      if (!generatorConfig) {
        try {
          generatorConfig = (await input.configProvider.getRootConfig(sessionRecord.cwd)).config
            .sessionTitles?.generator
        } catch {}
      }

      const preparedTitle = prepareSessionTitle(params.prompt, generatorConfig)
      if (preparedTitle.titleState === "placeholder" || !preparedTitle.promptText) {
        input.debug("session.titles.preparation_skipped", {
          sessionId: params.id,
          reason: "placeholder_result",
          titleState: preparedTitle.titleState,
        })
        return
      }

      input.updateSession(
        params.id,
        {
          title: preparedTitle.title,
          titleState: preparedTitle.titleState,
        },
        undefined,
        params.diagnosticLogger,
      )
      input.debug("session.titles.prepared", {
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
          diagnosticLogger: params.diagnosticLogger,
        })
      }
    })()
      .catch((error) => {
        input.emitDiagnostic(
          params.id,
          "session_title_generation_failed",
          {
            errorMessage: getErrorMessage(error),
          },
          params.diagnosticLogger,
        )
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
