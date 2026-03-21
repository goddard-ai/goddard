import { createBackendClient, type BackendClient } from "@goddard-ai/backend-client"
import type { RepoEvent, StreamMessage } from "@goddard-ai/schema/backend"
import { FileTokenStorage, RepoEventCursorStorage } from "@goddard-ai/storage"
import { resolveDaemonRuntimeConfig } from "./config.js"
import { buildPrompt, isFeedbackEvent } from "./feedback.js"
import { startDaemonServer, type DaemonServer } from "./ipc.js"
import {
  configureDaemonLogging,
  createDaemonLogger,
  createPayloadPreview,
  type DaemonLogMode,
} from "./logging.js"
import { runOneShot, type OneShotInput } from "./one-shot.js"

/** Input used to start the long-running daemon process. */
export type RunDaemonInput = {
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
  readRepoEventCursor?: (githubUsername: string) => Promise<number | undefined>
  writeRepoEventCursor?: (githubUsername: string, eventId: number) => Promise<void>
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
    mode: input.logMode ?? "pretty",
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
  const readRepoEventCursor =
    deps.readRepoEventCursor ??
    (async (githubUsername: string) =>
      (await RepoEventCursorStorage.get(githubUsername))?.lastEventId)
  const writeRepoEventCursor =
    deps.writeRepoEventCursor ??
    (async (githubUsername: string, eventId: number) => {
      await RepoEventCursorStorage.upsert(githubUsername, eventId)
    })
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
      ipcServer = await startIpcServer(client, {
        socketPath: runtime.socketPath,
        agentBinDir: runtime.agentBinDir,
      })
    }

    const activeIpcServer = ipcServer
    const runningPrs = new Set<string>()
    let subscription = null as Awaited<ReturnType<BackendClient["stream"]["subscribe"]>> | null

    if (enableStream) {
      const session = await client.auth.whoami()
      let currentCursor = (await readRepoEventCursor(session.githubUsername)) ?? 0
      let replaying = true
      let streamBlocked = false
      let eventQueue = Promise.resolve()
      const bufferedMessages: StreamMessage[] = []

      const processMessage = async (message: StreamMessage): Promise<void> => {
        if (!message.id || message.id <= currentCursor || streamBlocked) {
          return
        }

        try {
          await handleRepoEvent({
            event: message.event,
            client,
            activeIpcServer,
            agentBinDir: runtime.agentBinDir,
            logger,
            runOneShotImpl,
            runningPrs,
          })
          currentCursor = message.id
          await writeRepoEventCursor(session.githubUsername, message.id)
        } catch (error) {
          streamBlocked = true
          logger.log("repo.processing_blocked", {
            eventId: message.id,
            errorMessage: error instanceof Error ? error.message : String(error),
          })
        }
      }

      subscription = await client.stream.subscribe()
      logger.log(
        "repo.subscription_started",
        activeIpcServer
          ? {
              daemonUrl: activeIpcServer.daemonUrl,
              socketPath: activeIpcServer.socketPath,
            }
          : {},
      )

      subscription.on("message", (payload) => {
        const message = payload as StreamMessage
        if (replaying) {
          bufferedMessages.push(message)
          return
        }

        eventQueue = eventQueue.then(() => processMessage(message))
      })

      const history = await client.stream.history(currentCursor > 0 ? { after: currentCursor } : {})
      eventQueue = eventQueue.then(async () => {
        for (const record of history) {
          await processMessage({ id: record.id, event: record.event })
        }
      })

      replaying = false
      eventQueue = eventQueue.then(async () => {
        for (const message of bufferedMessages) {
          await processMessage(message)
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

/** Handles one managed pull-request stream event and launches daemon work when eligible. */
async function handleRepoEvent(input: {
  event: RepoEvent
  client: BackendClient
  activeIpcServer: DaemonServer | undefined
  agentBinDir: string
  logger: ReturnType<typeof createDaemonLogger>
  runOneShotImpl: (input: OneShotInput) => Promise<number> | number
  runningPrs: Set<string>
}): Promise<void> {
  const { event, client, activeIpcServer, agentBinDir, logger, runOneShotImpl, runningPrs } = input
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
      daemonUrl: activeIpcServer.daemonUrl,
      agentBinDir,
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
    throw error
  } finally {
    runningPrs.delete(requestKey)
  }
}
