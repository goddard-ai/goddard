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
      return
    }

    const task = (async () => {
      const sessionRecord = input.db.sessions.get(params.id) ?? null
      if (!sessionRecord || sessionRecord.titleState !== "pending") {
        return
      }

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
      return
    }

    const task = (async () => {
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
