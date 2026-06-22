import { createDaemonEventBus, type BackendEventHandler } from "@goddard-ai/daemon-plugin"
import { createLogStore, subtractHours, toErrorProperties } from "@goddard-ai/logs"
import { getErrorMessage } from "radashi"

import {
  createBackendClient,
  isBackendUnauthenticatedError,
  type BackendClient,
} from "./backend.ts"
import { createConfigManager } from "./config-manager.ts"
import { resolveRuntimeConfig } from "./config.ts"
import { SetupContext } from "./context.ts"
import { daemonRuntimeEvents } from "./events.ts"
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
    const daemonEvents = activeIpcServer?.events ?? createDaemonEventBus(daemonRuntimeEvents)
    const backendEventHandlers = createBackendEventHandlerRegistry()
    let subscription: Awaited<ReturnType<BackendClient["stream"]["subscribe"]>> | null = null
    let startingSubscription: Promise<void> | null = null

    if (activeIpcServer) {
      for (const handler of activeIpcServer.backendEventHandlers) {
        backendEventHandlers.register(handler)
      }
    }

    async function closeBackendStream() {
      if (!subscription) {
        return
      }

      const closingSubscription = subscription
      subscription = null
      await Promise.resolve(closingSubscription.close()).catch((error) => {
        logger.log("backend.stream_close_failed", toErrorProperties(error))
      })
    }

    async function startBackendStream() {
      if (!enableStream || subscription || startingSubscription || !backendEventHandlers.hasAny()) {
        return
      }

      startingSubscription = Promise.resolve()
        .then(async () => {
          try {
            subscription = await client.stream.subscribe()
          } catch (error) {
            const authError = error instanceof Error ? error : new Error(getErrorMessage(error))
            if (!isBackendUnauthenticatedError(authError)) {
              throw authError
            }

            logger.log("backend.stream_degraded", {
              reason: "unauthenticated",
              errorMessage: authError.message,
            })
            await daemonEvents.emit("backend.stream.degraded", {
              reason: "unauthenticated",
              errorMessage: authError.message,
            })
            return
          }

          logger.log(
            "backend.stream_started",
            activeIpcServer
              ? {
                  daemonUrl: activeIpcServer.daemonUrl,
                  port: activeIpcServer.port,
                }
              : {},
          )

          subscription.on("event", (payload) => {
            void dispatchBackendEvent(payload, backendEventHandlers, logger)
          })
        })
        .finally(() => {
          startingSubscription = null
        })

      await startingSubscription
    }

    backendEventHandlers.onHandlersChanged(() => {
      if (!backendEventHandlers.hasAny()) {
        void closeBackendStream()
        return
      }

      void startBackendStream()
    })

    if (enableStream) {
      await startBackendStream()
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

type BackendEventHandlerRegistry = {
  readonly getHandlers: () => readonly BackendEventHandler<any>[]
  readonly hasAny: () => boolean
  readonly onHandlersChanged: (listener: () => void) => () => void
  readonly register: (handler: BackendEventHandler<any>) => () => void
}

function createBackendEventHandlerRegistry(): BackendEventHandlerRegistry {
  const handlers = new Set<BackendEventHandler<any>>()
  const listeners = new Set<() => void>()

  function notifyChanged() {
    for (const listener of listeners) {
      listener()
    }
  }

  return {
    getHandlers: () => [...handlers],
    hasAny: () => handlers.size > 0,
    onHandlersChanged(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    register(handler) {
      handlers.add(handler)
      notifyChanged()
      return () => {
        if (handlers.delete(handler)) {
          notifyChanged()
        }
      }
    },
  }
}

async function dispatchBackendEvent(
  payload: unknown,
  handlers: BackendEventHandlerRegistry,
  logger: DaemonLogger,
) {
  for (const handler of handlers.getHandlers()) {
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
