import { createBackendClient, type BackendClient } from "@goddard-ai/backend/client"
import type { RepoEvent } from "@goddard-ai/schema/backend"
import { FileTokenStorage } from "@goddard-ai/storage"
import { resolveDaemonRuntimeConfig } from "./config.ts"
import { buildPrompt, isFeedbackEvent } from "./feedback.ts"
import { startDaemonServer, type DaemonServer } from "./ipc.ts"
import {
  configureDaemonLogging,
  createDaemonLogger,
  createPayloadPreview,
  type DaemonLogMode,
} from "./logging.ts"
import { runOneShot, type OneShotInput } from "./one-shot.ts"

/** Input used to start the long-running daemon process. */
export type RunDaemonInput = {
  projectDir: string
  baseUrl: string
  socketPath?: string
  agentBinDir?: string
  enableIpc?: boolean
  enableStream?: boolean
  logMode?: DaemonLogMode
}

/** Output sinks used by the daemon for structured log lines. */
export type DaemonIo = {
  stdout: (line: string) => void
  stderr: (line: string) => void
}

/** Optional test seams and runtime overrides for daemon startup. */
export type RunDaemonDeps = {
  createBackendClient?: (baseUrl: string) => Promise<BackendClient> | BackendClient
  startIpcServer?: (
    client: BackendClient,
    options: { socketPath: string; agentBinDir: string },
  ) => Promise<DaemonServer>
  runOneShot?: (input: OneShotInput) => Promise<number> | number
  waitForShutdown?: (close: () => void | Promise<void>) => Promise<void>
  io?: DaemonIo
}

const defaultIo: DaemonIo = {
  stdout: (line) => process.stdout.write(`${line}\n`),
  stderr: (line) => process.stderr.write(`${line}\n`),
}

/** Starts the daemon with the requested runtime features and waits for shutdown. */
export async function runDaemon(input: RunDaemonInput, deps: RunDaemonDeps = {}): Promise<number> {
  const io = deps.io ?? defaultIo
  const restoreLogging = configureDaemonLogging({
    writeLine: io.stdout,
    mode: input.logMode ?? "json",
  })
  const logger = createDaemonLogger()
  const enableIpc = input.enableIpc ?? true
  const enableStream = input.enableStream ?? true
  const runtime = resolveDaemonRuntimeConfig({
    baseUrl: input.baseUrl,
    socketPath: input.socketPath,
    agentBinDir: input.agentBinDir,
  })
  const createBackendClientImpl = deps.createBackendClient ?? defaultCreateBackendClient
  const startIpcServer =
    deps.startIpcServer ??
    ((client, options) =>
      startDaemonServer(client, {
        socketPath: options.socketPath,
        agentBinDir: options.agentBinDir,
      }))
  const runOneShotImpl = deps.runOneShot ?? runOneShot
  const waitForShutdownImpl = deps.waitForShutdown ?? waitForShutdown
  let ipcServer: DaemonServer | undefined

  try {
    logger.log("daemon.startup", {
      projectDir: input.projectDir,
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
      ipcServer = await startIpcServer(client, {
        socketPath: runtime.socketPath,
        agentBinDir: runtime.agentBinDir,
      })
    }

    const activeIpcServer = ipcServer
    const runningPrs = new Set<string>()
    const subscription = enableStream ? await client.stream.subscribe() : null

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

        if (!activeIpcServer) {
          logger.log("repo.feedback_ignored", {
            repository: `${event.owner}/${event.repo}`,
            prNumber: event.prNumber,
            feedbackType: event.type,
            reason: "ipc_disabled",
          })
          return
        }

        const prompt = buildPrompt(event)
        const requestKey = `${event.owner}/${event.repo}#${event.prNumber}`

        if (runningPrs.has(requestKey)) {
          logger.log("repo.feedback_coalesced", {
            repository: `${event.owner}/${event.repo}`,
            prNumber: event.prNumber,
            feedbackType: event.type,
          })
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
              repository: `${event.owner}/${event.repo}`,
              prNumber: event.prNumber,
              feedbackType: event.type,
              reason: "unmanaged_pr",
            })
            return
          }

          logger.log("one_shot.launch", {
            repository: `${event.owner}/${event.repo}`,
            prNumber: event.prNumber,
            feedbackType: event.type,
            prompt: createPayloadPreview(prompt),
          })
          const exitCode = await runOneShotImpl({
            event,
            prompt,
            projectDir: input.projectDir,
            daemonUrl: activeIpcServer.daemonUrl,
            agentBinDir: runtime.agentBinDir,
          })
          logger.log("one_shot.finish", {
            repository: `${event.owner}/${event.repo}`,
            prNumber: event.prNumber,
            feedbackType: event.type,
            exitCode,
          })
        } catch (error) {
          logger.log("one_shot.failed", {
            repository: `${event.owner}/${event.repo}`,
            prNumber: event.prNumber,
            feedbackType: event.type,
            errorMessage: error instanceof Error ? error.message : String(error),
          })
        } finally {
          runningPrs.delete(requestKey)
        }
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

async function defaultCreateBackendClient(baseUrl: string): Promise<BackendClient> {
  return createBackendClient({ baseUrl, tokenStorage: new FileTokenStorage() })
}
