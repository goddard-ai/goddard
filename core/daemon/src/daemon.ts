import { createLogStore, subtractHours, toErrorProperties } from "@goddard-ai/logs"
import { createRemoteRepoBackendEvent } from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import { getErrorMessage } from "radashi"

import { isBackendUnauthenticatedError } from "./backend.ts"
import { DaemonRuntimeEventBus } from "./events.ts"
import { startDaemonServer, type DaemonServer } from "./ipc.ts"
import {
  configureLogging,
  createLogger,
  installDaemonFatalErrorCapture,
  type DaemonLogger,
  type LogMode,
} from "./logging.ts"
import { type ComposedDaemonStore } from "./plugins.ts"
import { createDaemonRuntime, type DaemonRuntime } from "./runtime.ts"

/** Input used to start the long-running daemon process. */
export type RunInput = {
  baseUrl?: string
  port?: number
  agentBinDir?: string
  gitLibgit2Path?: string
  enableIpc?: boolean
  enableStream?: boolean
  logMode?: LogMode
  store?: ComposedDaemonStore
}

/** Starts the daemon with the requested runtime features and waits for shutdown. */
export async function runDaemon({
  baseUrl,
  port,
  agentBinDir,
  gitLibgit2Path,
  enableIpc = true,
  enableStream = true,
  logMode = "compact",
  store,
}: RunInput): Promise<number> {
  const logStore = createLogStore()
  const restoreLogging = configureLogging({
    mode: logMode,
    writeLine: (line) => {
      process.stdout.write(`${line}\n`)
    },
    store: logStore,
  })
  installDaemonFatalErrorCapture()
  const logger = createLogger()

  try {
    return await runConfiguredDaemon({
      agentBinDir,
      baseUrl,
      enableIpc,
      enableStream,
      logger,
      logStore,
      port,
      gitLibgit2Path,
      store,
    })
  } catch (error) {
    logger.log("daemon.run_failed", toErrorProperties(error))
    return 1
  } finally {
    restoreLogging()
    logStore.close()
  }
}

async function runConfiguredDaemon({
  agentBinDir,
  baseUrl,
  enableIpc,
  enableStream,
  logger,
  logStore,
  port,
  gitLibgit2Path,
  store,
}: {
  agentBinDir?: string
  baseUrl?: string
  enableIpc: boolean
  enableStream: boolean
  logger: DaemonLogger
  logStore: ReturnType<typeof createLogStore>
  port?: number
  gitLibgit2Path?: string
  store?: ComposedDaemonStore
}): Promise<number> {
  let ipcServer: DaemonServer | undefined
  let daemonRuntime: DaemonRuntime | undefined

  try {
    daemonRuntime = await createDaemonRuntime({
      agentBinDir,
      baseUrl,
      port,
      gitLibgit2Path,
      store,
    })
    const runtime = daemonRuntime
    logger.log("daemon.startup", runtime.runtimeConfig)
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

    if (enableIpc) {
      ipcServer = await startDaemonServer(runtime)
    }

    const daemonEvents: Pick<DaemonRuntimeEventBus, "emit"> = runtime.events

    const eventStreamAbort = new AbortController()
    let eventStreamTask: Promise<void> | null = null

    if (enableStream && ipcServer && runtime.backendEventHandlers.length > 0) {
      const { daemonUrl, port } = ipcServer
      const { backendEventHandlers } = runtime

      eventStreamTask = Promise.resolve()
        .then(async () => {
          const events = await runtime.client.events.stream(
            {
              names: ["comment", "review"],
            },
            {
              signal: eventStreamAbort.signal,
            },
          )
          await daemonEvents.emit("backend.stream.started", {
            daemonUrl,
            port,
          })

          await consumeBackendEvents(
            events,
            async (event) => {
              const payload = createRemoteRepoBackendEvent(event)
              for (const handler of backendEventHandlers) {
                try {
                  if (handler.canHandle(payload)) {
                    await handler.handle(payload)
                  }
                } catch (error) {
                  logger.log("backend.event_handler_failed", {
                    handlerName: handler.name,
                    ...toErrorProperties(error),
                  })
                }
              }
            },
            (error) => {
              logger.log("backend.stream.event_failed", toErrorProperties(error))
            },
            eventStreamAbort.signal,
          )
        })
        .catch(async (error) => {
          if (eventStreamAbort.signal.aborted) {
            return
          }

          const authError = error instanceof Error ? error : new Error(getErrorMessage(error))
          if (isBackendUnauthenticatedError(authError)) {
            await daemonEvents.emit("backend.stream.degraded", {
              reason: "unauthenticated",
              errorMessage: authError.message,
            })
            return
          }

          await daemonEvents.emit("backend.stream.degraded", {
            reason: "stream_failed",
            errorMessage: getErrorMessage(error),
          })
        })
    }

    await waitForShutdown(() =>
      Promise.all([
        eventStreamTask
          ? Promise.resolve()
              .then(() => eventStreamAbort.abort())
              .then(() => eventStreamTask)
              .catch(() => {})
          : Promise.resolve(),
        ipcServer ? ipcServer.close() : Promise.resolve(),
      ]).then(() => {}),
    )
    logger.log("daemon.shutdown", {
      port: ipcServer?.port ?? runtime.runtimeConfig.port,
    })
    return 0
  } finally {
    if (ipcServer) {
      await ipcServer.close().catch(() => {})
    }
    if (daemonRuntime) {
      await daemonRuntime.close().catch(() => {})
    }
  }
}

async function consumeBackendEvents(
  events: AsyncIterable<RepoEvent>,
  handleEvent: (event: RepoEvent) => Promise<void>,
  handleError: (error: unknown) => void,
  signal: AbortSignal,
) {
  const iterator = events[Symbol.asyncIterator]()
  const abort = () => {
    void iterator.return?.()
  }

  signal.addEventListener("abort", abort, { once: true })
  try {
    while (true) {
      const { done, value } = await iterator.next()
      if (done) {
        return
      }

      try {
        await handleEvent(value)
      } catch (error) {
        handleError(error)
      }
    }
  } finally {
    signal.removeEventListener("abort", abort)
    await iterator.return?.()
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
