import { createDaemonEventBus } from "@goddard-ai/daemon-plugin"
import { createLogStore, subtractHours, toErrorProperties } from "@goddard-ai/logs"
import { createRemoteRepoBackendEvent } from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import { getErrorMessage } from "radashi"

import {
  createBackendClient,
  isBackendUnauthenticatedError,
  type BackendClient,
} from "./backend.ts"
import { createConfigManager } from "./config-manager.ts"
import { resolveRuntimeConfig } from "./config.ts"
import { SetupContext } from "./context.ts"
import { daemonRuntimeEvents, type ConfigReloadFailedEvent } from "./events.ts"
import { startDaemonServer, type DaemonServer } from "./ipc.ts"
import {
  configureLogging,
  createLogger,
  installDaemonFatalErrorCapture,
  type DaemonLogger,
  type LogMode,
} from "./logging.ts"
import { openComposedDaemonStore, type ComposedDaemonStore } from "./plugins.ts"

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
export async function runDaemon({
  baseUrl,
  port,
  agentBinDir,
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
    const runtime = resolveRuntimeConfig({
      baseUrl,
      port,
      agentBinDir,
    })
    return await runConfiguredDaemon({
      ...runtime,
      enableIpc,
      enableStream,
      logger,
      logStore,
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
  store,
}: {
  agentBinDir: string
  baseUrl: string
  enableIpc: boolean
  enableStream: boolean
  logger: DaemonLogger
  logStore: ReturnType<typeof createLogStore>
  port: number
  store?: ComposedDaemonStore
}): Promise<number> {
  let emitConfigReloadFailed: ((event: ConfigReloadFailedEvent) => void | Promise<void>) | undefined
  const configManager = createConfigManager({
    onReloadFailed: (event) => emitConfigReloadFailed?.(event),
  })
  const ownsStore = store == null
  store ??= openComposedDaemonStore()
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
    const daemonEvents = activeIpcServer?.events ?? createDaemonEventBus(daemonRuntimeEvents)
    emitConfigReloadFailed = (event) => daemonEvents.emit("config.reload.failed", event)
    const eventStreamAbort = new AbortController()
    let eventStreamTask: Promise<void> | null = null

    if (enableStream && activeIpcServer && activeIpcServer.backendEventHandlers.length > 0) {
      eventStreamTask = Promise.resolve()
        .then(async () => {
          const events = await client.events.stream(
            {
              names: ["comment", "review"],
            },
            {
              signal: eventStreamAbort.signal,
            },
          )
          await daemonEvents.emit("backend.stream.started", {
            daemonUrl: activeIpcServer.daemonUrl,
            port: activeIpcServer.port,
          })

          await consumeBackendEvents(
            events,
            async (event) => {
              await dispatchBackendEvent(
                createRemoteRepoBackendEvent(event),
                activeIpcServer,
                logger,
              )
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

async function dispatchBackendEvent(
  payload: unknown,
  server: DaemonServer | undefined,
  logger: DaemonLogger,
) {
  if (!server) {
    return
  }

  for (const handler of server.backendEventHandlers) {
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
