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
  const enableIpc = input.enableIpc ?? true
  const enableStream = input.enableStream ?? true
  let configManager: ReturnType<typeof createConfigManager> | undefined
  let store: ComposedDaemonStore | undefined
  let ownsStore = false
  let ipcServer: DaemonServer | undefined

  try {
    const runtime = resolveRuntimeConfig({
      baseUrl: input.baseUrl,
      port: input.port,
      agentBinDir: input.agentBinDir,
    })
    configManager = createConfigManager()
    store = input.store ?? openComposedDaemonStore()
    ownsStore = input.store == null
    const activeConfigManager = configManager
    const activeStore = store

    logger.log("daemon.startup", {
      baseUrl: runtime.baseUrl,
      port: runtime.port,
      agentBinDir: runtime.agentBinDir,
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

    const client = await defaultCreateBackendClient(runtime.baseUrl, activeStore)
    if (enableIpc) {
      ipcServer = await SetupContext.run({ runtime, configManager: activeConfigManager }, () =>
        startDaemonServer(client, {
          port: runtime.port,
          agentBinDir: runtime.agentBinDir,
          store: activeStore,
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
                agentBinDir: runtime.agentBinDir,
                configManager: activeConfigManager,
                store: activeStore,
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
      port: ipcServer?.port ?? runtime.port,
    })
    return 0
  } catch (error) {
    logger.log("daemon.run_failed", toErrorProperties(error))
    return 1
  } finally {
    if (ipcServer) {
      await ipcServer.close().catch(() => {})
    }
    await configManager?.close().catch(() => {})
    if (ownsStore && store) {
      store.close()
    }
    restoreLogging()
    logStore.close()
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
