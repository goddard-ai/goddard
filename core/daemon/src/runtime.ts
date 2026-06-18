import {
  createEventBus,
  type BackendEventHandler,
  type DaemonConfigProvider,
  type DaemonSetupSubstrate,
  type EventBus,
  type DaemonEventEnvelope as PluginDaemonEventEnvelope,
} from "@goddard-ai/daemon-plugin"
import { toErrorProperties } from "@goddard-ai/logs"
import { getErrorMessage, isObject } from "radashi"

import { createBackendClient, type BackendClient } from "./backend.ts"
import { createConfigManager, type ConfigManager } from "./config-manager.ts"
import {
  prependAgentBinToPath,
  resolveRuntimeConfig,
  type ResolvedRuntimeConfig,
} from "./config.ts"
import { IpcRequestContext, SessionContext } from "./context.ts"
import { daemonRuntimeEvents } from "./events.ts"
import {
  createChunkPreview,
  createDebug,
  createLogger,
  createPayloadPreview,
  isVerboseLogging,
  type DaemonDebugLogger,
  type DaemonLogger,
} from "./logging.ts"
import {
  getDaemonPluginComposition,
  openComposedDaemonStore,
  type ComposedDaemonStore,
} from "./plugins.ts"

type ComposedDaemonPlugin = ReturnType<typeof getDaemonPluginComposition>["plugins"][number]

export type DaemonRuntime = {
  backendEventHandlers: readonly BackendEventHandler<any>[]
  client: BackendClient
  configManager: ConfigManager
  events: EventBus
  ipcHandlers: Record<string, unknown>
  ipcRoutes: Record<string, unknown>
  runtimeConfig: ResolvedRuntimeConfig
  store: ComposedDaemonStore
  close: () => Promise<void>
  setDaemonUrl: (daemonUrl: string) => void
}

export async function createDaemonRuntime(
  options: {
    agentBinDir?: string
    backendClient?: BackendClient
    baseUrl?: string
    configManager?: ConfigManager
    idleSessionShutdownTimeoutMs?: number
    port?: number
    reviewSyncLibgit2Path?: string
    store?: ComposedDaemonStore
  } = {},
): Promise<DaemonRuntime> {
  const logger = createLogger()
  const runtime = resolveRuntimeConfig({
    agentBinDir: options.agentBinDir,
    baseUrl: options.baseUrl,
    port: options.port,
    reviewSyncLibgit2Path: options.reviewSyncLibgit2Path,
  })
  const store =
    options.store ??
    openComposedDaemonStore(undefined, ({ error, filename }) => {
      logger.log("daemon.store_recreated", {
        filename,
        ...toErrorProperties(error),
      })
    })
  const metadataStore = store.metadata as {
    get: (key: string) => unknown
    set: (key: string, value: unknown) => void
    delete: (key: string) => void
  }
  const composition = getDaemonPluginComposition()
  const events = createEventBus<any>({
    ...daemonRuntimeEvents,
    ...composition.events,
  })
  observeDaemonEventsForLogging(events, logger)
  const configManager =
    options.configManager ??
    createConfigManager({
      onReloadFailed: (event) => events.emit("config.reload.failed", event),
    })
  const client =
    options.backendClient ??
    createBackendClient({
      baseUrl: runtime.baseUrl,
      getAuthorizationHeader: async () => {
        const token = store.metadata.get("authToken") ?? null
        return token ? `Bearer ${token}` : null
      },
    })

  let daemonUrl: string | undefined

  function requireIpcRequestContext() {
    const context = IpcRequestContext.get()
    if (!context) {
      throw new Error("IPC request context is unavailable")
    }

    return context
  }

  const daemonSubstrate = {
    daemonRuntime: {
      agentBinDir: runtime.agentBinDir,
      idleSessionShutdownTimeoutMs: options.idleSessionShutdownTimeoutMs,
      getDaemonUrl() {
        if (!daemonUrl) {
          throw new Error("Daemon URL is unavailable before startup completes")
        }
        return daemonUrl
      },
      createAgentEnvironment({ env }) {
        return prependAgentBinToPath(runtime.agentBinDir, env)
      },
    },
    authTokenStore: {
      set: (token) => {
        store.metadata.set("authToken", token)
      },
      delete: () => {
        store.metadata.delete("authToken")
      },
    },
    log: {
      createDebug,
      createLogger,
      isVerboseLogging,
      createPayloadPreview,
      createChunkPreview,
    },
    metadataStore: {
      get: (key) => metadataStore.get(key),
      set: (key, value) => {
        metadataStore.set(key, value)
      },
      delete: (key) => {
        metadataStore.delete(key)
      },
    },
    sessionContext: {
      run: (context, callback) => SessionContext.run(context, callback),
    },
    ipc: {
      get requestContext() {
        return requireIpcRequestContext()
      },
    },
  } satisfies DaemonSetupSubstrate

  const pluginSetup = await setupDaemonPlugins(
    daemonSubstrate,
    {
      getRootConfig: configManager.getRootConfig,
      getLastKnownRootConfig: configManager.getLastKnownRootConfig,
    },
    client,
    store,
    composition.plugins,
    events,
  )
  let closed = false

  return {
    backendEventHandlers: pluginSetup.backendEventHandlers,
    client,
    configManager,
    events,
    ipcHandlers: pluginSetup.ipcHandlers,
    ipcRoutes: composition.ipcRoutes,
    runtimeConfig: runtime,
    store,
    setDaemonUrl(value) {
      daemonUrl = value
    },
    close: async () => {
      if (closed) {
        return
      }
      closed = true
      await pluginSetup.close().catch(() => {})
      await configManager.close().catch(() => {})
      store.close()
    },
  }
}

async function setupDaemonPlugins(
  substrate: DaemonSetupSubstrate,
  configProvider: DaemonConfigProvider,
  backendClient: BackendClient,
  store: ComposedDaemonStore,
  plugins: readonly ComposedDaemonPlugin[],
  events: EventBus,
) {
  const logger = createLogger()
  const debug = createDebug("daemon.plugins")
  const extensions: Record<string, unknown> = {}
  const backendEventHandlers: BackendEventHandler<any>[] = []
  const ipcHandlers: Record<string, unknown> = {}
  const closeHandlers: Array<{
    pluginName: string
    close: () => void | Promise<void>
  }> = []

  for (const plugin of plugins) {
    const startedAt = Date.now()
    debug("daemon.plugins.setup_started", {
      pluginName: plugin.name,
    })
    // Runtime setup intentionally shares full daemon resources; plugin isolation is type-level.
    const context = Object.assign(
      Object.create(substrate) as DaemonSetupSubstrate,
      {
        db: store,
        backend: backendClient,
        configProvider,
        events,
      },
      extensions,
    )
    let setup: Awaited<ReturnType<NonNullable<typeof plugin.setup>>>
    try {
      setup = await plugin.setup?.(context)
    } catch (error) {
      logger.log("daemon.plugin_setup_failed", {
        pluginName: plugin.name,
        durationMs: Date.now() - startedAt,
        ...toErrorProperties(error),
      })
      throw error
    }

    if (setup?.ipcHandlers) {
      mergeIpcHandlers(ipcHandlers, setup.ipcHandlers as Record<string, unknown>)
    }

    if (setup?.backendEventHandlers) {
      backendEventHandlers.push(...setup.backendEventHandlers)
    }

    if (setup?.close) {
      closeHandlers.push({
        pluginName: plugin.name,
        close: setup.close,
      })
    }

    if (setup?.provides) {
      Object.assign(extensions, setup.provides)
    }

    debug("daemon.plugins.setup_completed", {
      pluginName: plugin.name,
      durationMs: Date.now() - startedAt,
    })
  }

  return {
    backendEventHandlers,
    ipcHandlers,
    close: async () => {
      for (let index = closeHandlers.length - 1; index >= 0; index -= 1) {
        const closeHandler = closeHandlers[index]
        if (!closeHandler) {
          continue
        }

        const startedAt = Date.now()
        debug("daemon.plugins.close_started", {
          pluginName: closeHandler.pluginName,
        })
        try {
          await closeHandler.close()
        } catch (error) {
          logger.log("daemon.plugin_close_failed", {
            pluginName: closeHandler.pluginName,
            durationMs: Date.now() - startedAt,
            ...toErrorProperties(error),
          })
          throw error
        }
        debug("daemon.plugins.close_completed", {
          pluginName: closeHandler.pluginName,
          durationMs: Date.now() - startedAt,
        })
      }
    },
  }
}

function observeDaemonEventsForLogging(events: EventBus, logger: DaemonLogger) {
  const debugLoggers = new Map<string, DaemonDebugLogger>()

  events.observe((event) => {
    const fields = createDaemonEventLogFields(event)
    const debugScope = event.options?.debug
    if (debugScope) {
      const debugLogger =
        debugLoggers.get(debugScope) ?? createAndRememberDebugLogger(debugLoggers, debugScope)
      debugLogger(event.name, fields)
      return
    }

    logger.log(event.name, fields)
  })
}

function createAndRememberDebugLogger(loggers: Map<string, DaemonDebugLogger>, debugScope: string) {
  const logger = createDebug(debugScope)
  loggers.set(debugScope, logger)
  return logger
}

function createDaemonEventLogFields(event: PluginDaemonEventEnvelope): Record<string, unknown> {
  const payloadFields = isObject(event.payload) ? event.payload : { payload: event.payload }
  return {
    ...sanitizeEventLogFields(payloadFields),
    eventId: event.id,
    eventAt: event.at,
  }
}

function sanitizeEventLogFields(fields: object) {
  const sanitized: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(fields)) {
    sanitized[key] = value instanceof Error ? { errorMessage: getErrorMessage(value) } : value
  }
  return sanitized
}

function mergeIpcHandlers(target: Record<string, unknown>, source: Record<string, unknown>) {
  for (const [key, sourceValue] of Object.entries(source)) {
    const targetValue = target[key]
    if (!targetValue) {
      target[key] = sourceValue
      continue
    }

    if (
      typeof targetValue === "object" &&
      targetValue !== null &&
      typeof sourceValue === "object" &&
      sourceValue !== null &&
      typeof targetValue !== "function" &&
      typeof sourceValue !== "function"
    ) {
      mergeIpcHandlers(
        targetValue as Record<string, unknown>,
        sourceValue as Record<string, unknown>,
      )
      continue
    }

    throw new Error(`Duplicate IPC handler: ${key}`)
  }
}
