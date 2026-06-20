import { createLogStore, subtractHours, toErrorProperties } from "@goddard-ai/logs"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import { getErrorMessage } from "radashi"

import {
  createBackendClient,
  isBackendUnauthenticatedError,
  type BackendClient,
} from "./backend.ts"
import { createConfigManager } from "./config-manager.ts"
import { resolveRuntimeConfig } from "./config.ts"
import { FeedbackEventContext, SetupContext } from "./context.ts"
import { buildPrompt, isFeedbackEvent } from "./feedback.ts"
import { startDaemonServer, type DaemonServer } from "./ipc.ts"
import {
  configureLogging,
  createLogger,
  createPayloadPreview,
  installDaemonFatalErrorCapture,
  type DaemonLogger,
  type LogMode,
} from "./logging.ts"
import { openComposedDaemonStore, type ComposedDaemonStore } from "./plugins.ts"
import { runPrFeedbackFlow } from "./pr-feedback-run.ts"

/** Input used to start the long-running daemon process. */
export type RunInput = {
  baseUrl: string
  port?: number
  agentBinDir?: string
  enableIpc?: boolean
  enableStream?: boolean
  logMode?: LogMode
  store?: ComposedDaemonStore
}

type ConfiguredDaemonInput = {
  agentBinDir: string
  baseUrl: string
  configManager: ReturnType<typeof createConfigManager>
  enableIpc: boolean
  enableStream: boolean
  logger: DaemonLogger
  logStore: ReturnType<typeof createLogStore>
  ownsStore: boolean
  port: number
  store: ComposedDaemonStore
}

/** Starts the daemon with the requested runtime features and waits for shutdown. */
export async function runDaemon(input: RunInput): Promise<number> {
  const logStore = createLogStore()
  const restoreLogging = configureLogging({
    mode: input.logMode ?? "compact",
    writeLine: (line) => {
      process.stdout.write(`${line}\n`)
    },
    store: logStore,
  })
  installDaemonFatalErrorCapture()
  const logger = createLogger()

  try {
    const runtime = resolveRuntimeConfig({
      baseUrl: input.baseUrl,
      port: input.port,
      agentBinDir: input.agentBinDir,
    })
    const configManager = createConfigManager()
    let didHandoffConfigManager = false

    try {
      const store = input.store ?? openComposedDaemonStore()
      didHandoffConfigManager = true

      return await runConfiguredDaemon({
        ...runtime,
        configManager,
        enableIpc: input.enableIpc ?? true,
        enableStream: input.enableStream ?? true,
        logger,
        logStore,
        ownsStore: input.store == null,
        store,
      })
    } finally {
      if (!didHandoffConfigManager) {
        await configManager.close().catch(() => {})
      }
    }
  } catch (error) {
    logger.log("daemon.run_failed", toErrorProperties(error))
    return 1
  } finally {
    restoreLogging()
    logStore.close()
  }
}

async function runConfiguredDaemon(input: ConfiguredDaemonInput): Promise<number> {
  const {
    agentBinDir,
    baseUrl,
    configManager,
    enableIpc,
    enableStream,
    logger,
    logStore,
    ownsStore,
    port,
    store,
  } = input
  let ipcServer: DaemonServer | undefined

  try {
    logger.log("daemon.startup", {
      baseUrl,
      port,
      agentBinDir,
    })
    void Promise.resolve()
      .then(() => {
        logStore.retainSince(subtractHours(new Date(), 24))
      })
      .catch((error) => {
        logger.log("logs.retention_failed", {
          errorMessage: getErrorMessage(error),
        })
      })

    if (enableIpc === false && enableStream === false) {
      logger.log("daemon.no_features_enabled", {})
      return 0
    }

    const client = await defaultCreateBackendClient(baseUrl, store)
    if (enableIpc) {
      ipcServer = await SetupContext.run(
        { runtime: { agentBinDir, baseUrl, port }, configManager },
        () =>
          startDaemonServer(client, {
            port,
            agentBinDir,
            store,
          }),
      )
    }

    const activeIpcServer = ipcServer
    // Coalesce feedback per PR so one daemon run owns the repo state until it finishes.
    const runningPrs = new Set<string>()
    let subscription: Awaited<ReturnType<BackendClient["stream"]["subscribe"]>> | null = null

    if (enableStream) {
      try {
        subscription = await client.stream.subscribe()
      } catch (error) {
        const authError = error instanceof Error ? error : new Error(getErrorMessage(error))
        if (!isBackendUnauthenticatedError(authError)) {
          throw authError
        }

        logger.log("repo.subscription_degraded", {
          reason: "unauthenticated",
          errorMessage: authError.message,
        })
      }
    }

    if (subscription) {
      logger.log(
        "repo.subscription_started",
        activeIpcServer
          ? {
              daemonUrl: activeIpcServer.daemonUrl,
              port: activeIpcServer.port,
            }
          : {},
      )

      subscription.on("event", async (payload) => {
        try {
          const event = payload as RepoEvent
          if (!isFeedbackEvent(event)) {
            return
          }

          const feedbackContext = {
            repository: `${event.owner}/${event.repo}`,
            prNumber: event.prNumber,
            feedbackType: event.type,
          }

          await FeedbackEventContext.run(feedbackContext, async () => {
            if (!activeIpcServer) {
              logger.log("repo.feedback_ignored", {
                reason: "ipc_disabled",
              })
              return
            }

            const prompt = buildPrompt(event)
            const requestKey = `${event.owner}/${event.repo}#${event.prNumber}`

            if (runningPrs.has(requestKey)) {
              logger.log("repo.feedback_coalesced")
              return
            }

            runningPrs.add(requestKey)

            try {
              const { managed } = await client.pullRequests.managed({
                owner: event.owner,
                repo: event.repo,
                prNumber: event.prNumber,
              })
              if (!managed) {
                logger.log("repo.feedback_ignored", {
                  reason: "unmanaged_pr",
                })
                return
              }

              logger.log("pr_feedback.launch", {
                prompt: createPayloadPreview(prompt),
              })
              const exitCode = await runPrFeedbackFlow({
                event,
                prompt,
                daemonUrl: activeIpcServer.daemonUrl,
                agentBinDir,
                configManager,
                store,
              })
              logger.log("pr_feedback.finish", {
                exitCode,
              })
            } catch (error) {
              logger.log("pr_feedback.failed", toErrorProperties(error))
            } finally {
              runningPrs.delete(requestKey)
            }
          })
        } catch (error) {
          logger.log("repo.event_failed", toErrorProperties(error))
        }
      })
    }

    await waitForShutdown(() =>
      Promise.all([
        subscription ? Promise.resolve(subscription.close()) : Promise.resolve(),
        activeIpcServer ? activeIpcServer.close() : Promise.resolve(),
      ]).then(() => {}),
    )
    logger.log("daemon.shutdown", {
      port: ipcServer?.port ?? port,
    })
    return 0
  } finally {
    if (ipcServer) {
      await ipcServer.close().catch(() => {})
    }
    await configManager.close().catch(() => {})
    if (ownsStore) {
      store.close()
    }
  }
}

/** Waits for SIGINT and then closes the active daemon resources. */
export async function waitForShutdown(close: () => void | Promise<void>): Promise<void> {
  await new Promise<void>((resolve) => {
    process.once("SIGINT", () => {
      void close()
      resolve()
    })
  })
}

/** Creates the daemon-owned backend client with auth headers sourced from daemon persistence. */
async function defaultCreateBackendClient(
  baseUrl: string,
  store: ComposedDaemonStore,
): Promise<BackendClient> {
  return createBackendClient({
    baseUrl,
    getAuthorizationHeader: async () => {
      const token = store.metadata.get("authToken") ?? null
      return token ? `Bearer ${token}` : null
    },
  })
}
