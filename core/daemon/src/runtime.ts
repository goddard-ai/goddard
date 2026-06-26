import { composeBackendRoutes } from "@goddard-ai/backend-plugin"
import {
  createEventBus,
  type BackendEventHandler,
  type DaemonConfigProvider,
  type DaemonSetupSubstrate,
  type EventBus,
  type DaemonEventEnvelope as PluginDaemonEventEnvelope,
} from "@goddard-ai/daemon-plugin"
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
    store?: ComposedDaemonStore
  } = {},
): Promise<DaemonRuntime> {
  const logger = createLogger()
  const runtime = resolveRuntimeConfig({
    agentBinDir: options.agentBinDir,
    baseUrl: options.baseUrl,
    port: options.port,
  })
  const store = options.store ?? openComposedDaemonStore()
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
  const extensions: Record<string, unknown> = {}
  const extensionsByPluginName = new Map<string, Record<string, unknown>>()
  const backendEventHandlers: BackendEventHandler<any>[] = []
  const ipcHandlers: Record<string, unknown> = {}
  const closeHandlers: Array<() => void | Promise<void>> = []

  for (const plugin of plugins) {
    const consumedExtensions = (plugin.consumes ?? []).map(
      (consumedPlugin) => extensionsByPluginName.get(consumedPlugin.name) ?? {},
    )
    const context = Object.assign(
      Object.create(substrate) as DaemonSetupSubstrate,
      {
        db: store,
        backend: createPluginBackendContext(plugin, backendClient),
        configProvider: createPluginConfigProvider(configProvider, plugin),
        events,
      },
      ...consumedExtensions,
    )
    const setup = await plugin.setup?.(context)

    if (setup?.ipcHandlers) {
      mergeIpcHandlers(ipcHandlers, setup.ipcHandlers as Record<string, unknown>)
    }

    if (setup?.backendEventHandlers) {
      backendEventHandlers.push(...setup.backendEventHandlers)
    }

    if (setup?.close) {
      closeHandlers.push(setup.close)
    }

    if (setup?.provides) {
      Object.assign(extensions, setup.provides)
      extensionsByPluginName.set(plugin.name, setup.provides)
    }
  }

  return {
    backendEventHandlers,
    extensions,
    ipcHandlers,
    close: async () => {
      for (let index = closeHandlers.length - 1; index >= 0; index -= 1) {
        await closeHandlers[index]?.()
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

function createPluginBackendContext(plugin: ComposedDaemonPlugin, client: BackendClient) {
  const routes = composeBackendRoutes([
    ...(plugin.consumes ?? []).map((consumedPlugin) => consumedPlugin.backendRoutes ?? {}),
    plugin.backendRoutes ?? {},
  ])

  return selectBackendClientRoutes(routes, client)
}

function createPluginConfigProvider(
  source: DaemonConfigProvider,
  plugin: ComposedDaemonPlugin,
): DaemonConfigProvider {
  const keys = new Set([
    "agents",
    "registry",
    "security",
    "session",
    ...getPluginConfigKeys(plugin),
  ])

  return {
    async getRootConfig(cwd) {
      const snapshot = await source.getRootConfig(cwd)
      return {
        ...snapshot,
        config: selectConfigKeys(snapshot.config, keys),
      }
    },
    getLastKnownRootConfig(cwd) {
      const snapshot = source.getLastKnownRootConfig(cwd)
      if (!snapshot) {
        return null
      }

      return {
        ...snapshot,
        config: selectConfigKeys(snapshot.config, keys),
      }
    },
  }
}

function getPluginConfigKeys(plugin: ComposedDaemonPlugin): string[] {
  const keys = [
    ...readConfigKeys(plugin),
    ...(plugin.consumes ?? []).flatMap((consumedPlugin) => readConfigKeys(consumedPlugin)),
  ]

  return keys
}

function readConfigKeys(plugin: ComposedDaemonPlugin) {
  if (!plugin.config) {
    return []
  }

  return Object.keys(plugin.config)
}

function selectConfigKeys(config: Record<string, unknown>, keys: ReadonlySet<string>) {
  const selected: Record<string, unknown> = {}

  for (const key of keys) {
    if (key in config) {
      selected[key] = config[key]
    }
  }

  return selected
}

function selectBackendClientRoutes(routes: Record<string, any>, source: Record<string, any>) {
  const context: Record<string, unknown> = {}

  for (const [key, route] of Object.entries(routes)) {
    if (!route || typeof route !== "object") {
      continue
    }
    if (route.kind === "resource") {
      context[key] = selectBackendClientRoutes(route.children, source[key])
      continue
    }
    context[key] = source[key]
  }

  return context
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
