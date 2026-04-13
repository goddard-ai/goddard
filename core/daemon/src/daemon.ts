import type { RepoEvent } from "@goddard-ai/schema/backend"

import {
  createBackendClient,
  type BackendClient,
  isBackendUnauthenticatedError,
} from "./backend.ts"
import { createConfigManager, type ConfigManager } from "./config-manager.ts"
import { resolveRuntimeConfig } from "./config.ts"
import { FeedbackEventContext, SetupContext } from "./context.ts"
import { buildPrompt, isFeedbackEvent } from "./feedback.ts"
import { startDaemonServer, type DaemonServer } from "./ipc.ts"
import { configureLogging, createLogger, createPayloadPreview, type LogMode } from "./logging.ts"
import { db } from "./persistence/store.ts"
import { runPrFeedbackFlow, type PrFeedbackFlowInput } from "./pr-feedback-run.ts"

/** Input used to start the long-running daemon process. */
export type RunInput = {
  baseUrl: string
  socketPath?: string
  agentBinDir?: string
  enableIpc?: boolean
  enableStream?: boolean
  logMode?: LogMode
}

/** Output sinks used by the daemon for structured log lines. */
export type Io = {
  stdout: (line: string) => void
  stderr: (line: string) => void
}

/** Optional test seams and runtime overrides for daemon startup. */
export type RunDeps = {
  createBackendClient?: (baseUrl: string) => Promise<BackendClient> | BackendClient
  createConfigManager?: () => ConfigManager
  startIpcServer?: (
    client: BackendClient,
    options: { socketPath: string; agentBinDir: string },
  ) => Promise<DaemonServer>
  runPrFeedbackFlow?: (input: PrFeedbackFlowInput) => Promise<number> | number
  waitForShutdown?: (close: () => void | Promise<void>) => Promise<void>
  io?: Io
}

const defaultIo: Io = {
  stdout: (line) => process.stdout.write(`${line}\n`),
  stderr: (line) => process.stderr.write(`${line}\n`),
}

/** Starts the daemon with the requested runtime features and waits for shutdown. */
export async function runDaemon(input: RunInput, deps: RunDeps = {}): Promise<number> {
  const io = deps.io ?? defaultIo
  const restoreLogging = configureLogging({
    writeLine: io.stdout,
    mode: input.logMode ?? "pretty",
  })
  const logger = createLogger()
  const enableIpc = input.enableIpc ?? true
  const enableStream = input.enableStream ?? true
  const runtime = resolveRuntimeConfig({
    baseUrl: input.baseUrl,
    socketPath: input.socketPath,
    agentBinDir: input.agentBinDir,
  })
  const createBackendClientImpl = deps.createBackendClient ?? defaultCreateBackendClient
  const configManager = (deps.createConfigManager ?? createConfigManager)()
  const startIpcServer =
    deps.startIpcServer ??
    ((client, options) =>
      startDaemonServer(client, {
        socketPath: options.socketPath,
        agentBinDir: options.agentBinDir,
      }))
  const runPrFeedbackFlowImpl =
    deps.runPrFeedbackFlow ??
    ((prFeedbackFlowInput) =>
      runPrFeedbackFlow({
        ...prFeedbackFlowInput,
        configManager,
      }))
  const waitForShutdownImpl = deps.waitForShutdown ?? waitForShutdown
  let ipcServer: DaemonServer | undefined

  try {
    logger.log("daemon.startup", {
      baseUrl: runtime.baseUrl,
      socketPath: runtime.socketPath,
      agentBinDir: runtime.agentBinDir,
    })

    if (enableIpc === false && enableStream === false) {
      logger.log("daemon.no_features_enabled", {})
      return 0
    }

    const client = await createBackendClientImpl(runtime.baseUrl)
    if (enableIpc) {
      ipcServer = await SetupContext.run({ runtime, configManager }, () =>
        startIpcServer(client, {
          socketPath: runtime.socketPath,
          agentBinDir: runtime.agentBinDir,
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
        const authError = error instanceof Error ? error : new Error(String(error))
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
              socketPath: activeIpcServer.socketPath,
            }
          : {},
      )

      subscription.on("event", async (payload) => {
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
            const managed = await client.pr.isManaged({
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
            const exitCode = await runPrFeedbackFlowImpl({
              event,
              prompt,
              daemonUrl: activeIpcServer.daemonUrl,
              agentBinDir: runtime.agentBinDir,
            })
            logger.log("pr_feedback.finish", {
              exitCode,
            })
          } catch (error) {
            logger.log("pr_feedback.failed", {
              errorMessage: error instanceof Error ? error.message : String(error),
            })
          } finally {
            runningPrs.delete(requestKey)
          }
        })
      })
    }

    await waitForShutdownImpl(() =>
      Promise.all([
        subscription ? Promise.resolve(subscription.close()) : Promise.resolve(),
        activeIpcServer ? activeIpcServer.close() : Promise.resolve(),
      ]).then(() => {}),
    )
    logger.log("daemon.shutdown", {
      socketPath: runtime.socketPath,
    })
    return 0
  } catch (error) {
    logger.log("daemon.run_failed", {
      errorMessage: error instanceof Error ? error.message : String(error),
    })
    return 1
  } finally {
    if (ipcServer) {
      await ipcServer.close().catch(() => {})
    }
    await configManager.close().catch(() => {})
    restoreLogging()
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
async function defaultCreateBackendClient(baseUrl: string): Promise<BackendClient> {
  return createBackendClient({
    baseUrl,
    getAuthorizationHeader: async () => {
      const token = db.metadata.get("authToken") ?? null
      return token ? `Bearer ${token}` : null
    },
    clearAuthorization: async () => {
      db.metadata.delete("authToken")
    },
  })
}
