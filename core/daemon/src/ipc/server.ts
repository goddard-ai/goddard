import { randomUUID } from "node:crypto"
import { once } from "node:events"
import type { Server } from "node:http"
import { composeBackendRoutes } from "@goddard-ai/backend-plugin"
import {
  createDaemonEventBus,
  type DaemonConfigProvider,
  type DaemonSetupSubstrate,
  type EventBus,
  type EventDefinition,
  type DaemonEventEnvelope as PluginDaemonEventEnvelope,
} from "@goddard-ai/daemon-plugin"
import { composeIpcRoutes } from "@goddard-ai/ipc"
import { createServer } from "@goddard-ai/ipc/node"
import type { ManagedAgentService } from "@goddard-ai/managed-agent/daemon"
import {
  coreDaemonIpcRoutes,
  type BrowserAccessClientRevokeRequest,
  type BrowserAccessPairingCompleteRequest,
  type BrowserAccessPairingConfirmRequest,
  type BrowserAccessPairingStartRequest,
  type BrowserAccessWebviewTokenCreateRequest,
  type DaemonEventsStreamRequest,
} from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { type DaemonSession } from "@goddard-ai/session/schema"
import { getErrorMessage, isObject } from "radashi"

import { createManagedAgentUpdateScheduler } from "../agent-update-scheduler.ts"
import type { BackendClient } from "../backend.ts"
import {
  createBrowserAccessService,
  resolveBrowserAccessRuntimeConfig,
  runBrowserAccessRequestContext,
} from "../browser-access.ts"
import { createConfigManager } from "../config-manager.ts"
import { prependAgentBinToPath, resolveRuntimeConfig } from "../config.ts"
import { IpcRequestContext, SessionContext, SetupContext } from "../context.ts"
import { daemonRuntimeEvents } from "../events.ts"
import {
  createChunkPreview,
  createDebug,
  createLogger,
  createPayloadPreview,
  isVerboseLogging,
  readSessionIdForLog,
  type DaemonDebugLogger,
  type DaemonLogger,
} from "../logging.ts"
import type { ManagedAgentUsageStore } from "../managed-agent-usage.ts"
import {
  getDaemonPluginComposition,
  openComposedDaemonStore,
  type ComposedDaemonStore,
} from "../plugins.ts"
import type { DaemonServer } from "./types.ts"

type ComposedDaemonPlugin = ReturnType<typeof getDaemonPluginComposition>["plugins"][number]
type ComposedDaemonEvents = EventBus<Record<string, EventDefinition<unknown>>>

export async function startDaemonServer(
  client: BackendClient,
  options: {
    port?: number
    agentBinDir?: string
    idleSessionShutdownTimeoutMs?: number
    store?: ComposedDaemonStore
  } = {},
): Promise<DaemonServer> {
  const logger = createLogger()
  const debug = createDebug("ipc.server")
  const setupContext = SetupContext.get()
  const runtime =
    setupContext?.runtime ??
    resolveRuntimeConfig({
      port: options.port,
      agentBinDir: options.agentBinDir,
    })
  const configManager = setupContext?.configManager ?? createConfigManager()
  const ownsConfigManager = setupContext == null
  const store = options.store ?? openComposedDaemonStore()
  const ownsStore = options.store == null
  const metadataStore = store.metadata as {
    get: (key: string) => unknown
    set: (key: string, value: unknown) => void
    delete: (key: string) => void
  }
  const rootConfig = await configManager.getRootConfig()
  const browserAccessConfig = resolveBrowserAccessRuntimeConfig(
    rootConfig.config.daemon?.browserAccess,
  )
  const browserAccessService = createBrowserAccessService(store, browserAccessConfig)
  const composition = getDaemonPluginComposition()
  const events = createDaemonEventBus({
    ...daemonRuntimeEvents,
    ...composition.events,
  })
  observeDaemonEventsForLogging(events, logger)

  const managedAgentUsageStore: ManagedAgentUsageStore = {
    get: () => store.metadata.get("managedAgentUsage") ?? {},
    set: (state) => {
      store.metadata.set("managedAgentUsage", state)
    },
  }
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
  const managedAgent = pluginSetup.extensions.managedAgent as ManagedAgentService | undefined
  if (!managedAgent) {
    throw new Error("Managed-agent plugin did not provide a managed-agent service.")
  }
  const agentUpdateScheduler = createManagedAgentUpdateScheduler({
    configProvider: {
      getRootConfig: configManager.getRootConfig,
      getLastKnownRootConfig: configManager.getLastKnownRootConfig,
    },
    agentInstallService: managedAgent,
    updateCheckStore: {
      get: () => store.metadata.get("managedAgentUpdateChecks") ?? {},
      set: (state) => {
        store.metadata.set("managedAgentUpdateChecks", state)
      },
    },
    usageStore: managedAgentUsageStore,
    logger,
  })
  const ipcHandlers = {
    daemon: {
      health: async () => ({ ok: true }),
      browserAccess: {
        pairing: {
          start: ({ body }: { body: BrowserAccessPairingStartRequest }) =>
            browserAccessService.startPairing(body),
          confirm: ({ body }: { body: BrowserAccessPairingConfirmRequest }) =>
            browserAccessService.confirmPairing(body),
          complete: ({ body }: { body: BrowserAccessPairingCompleteRequest }) =>
            browserAccessService.completePairing(body),
        },
        client: {
          list: () => browserAccessService.listClients(),
          revoke: ({ body }: { body: BrowserAccessClientRevokeRequest }) =>
            browserAccessService.revokeClient(body),
        },
        webviewToken: {
          create: ({ body }: { body: BrowserAccessWebviewTokenCreateRequest }) =>
            browserAccessService.createDesktopWebviewToken(body),
        },
      },
    },
    ...pluginSetup.ipcHandlers,
    events: {
      stream: (ctx: { body: DaemonEventsStreamRequest; request: Request }) => {
        return events.stream(ctx.body ?? {}, ctx.request.signal)
      },
    },
  }

  const ipcServer = createServer({
    port: runtime.port,
    routes: composeIpcRoutes([coreDaemonIpcRoutes, composition.ipcRoutes]),
    handlers: ipcHandlers as any,
    browserAccess: {
      allowedOrigins: browserAccessConfig.allowedOrigins,
      authorizeRequest: browserAccessService.authorizeRequest,
    },
    runHandler: ({ payload, request }, handler) => {
      const context: IpcRequestContext = {
        opId: randomUUID(),
        sessionId: readSessionIdForLog(payload) ?? null,
        setSessionId(sessionId: DaemonSession["id"]) {
          context.sessionId = sessionId
        },
      }
      return IpcRequestContext.run(context, () => runBrowserAccessRequestContext(request, handler))
    },
    onRequestReceived: ({ name, payload }) => {
      debug("ipc.request_received", {
        requestName: name,
        method: name,
        payload,
      })
    },
    onResponseSent: ({ name, response, durationMs }) => {
      const responseSessionId = readSessionIdForLog(response)
      if (responseSessionId) {
        const context = requireIpcRequestContext()
        context.setSessionId(responseSessionId)
      }

      debug("ipc.response_sent", {
        requestName: name,
        method: name,
        durationMs,
        response,
      })
    },
    onRequestFailed: ({ name, error, durationMs }) => {
      logger.log("ipc.request_failed", {
        requestName: name,
        method: name,
        durationMs,
        errorMessage: getErrorMessage(error),
      })
    },
  })

  await once(ipcServer.server, "listening")
  const port = readBoundTcpPort(ipcServer.server)
  daemonUrl = createDaemonUrl(port)

  logger.log("ipc.server_listening", {
    port,
    daemonUrl,
  })
  agentUpdateScheduler.start()

  let closed = false

  return {
    daemonUrl,
    events,
    port,
    close: async () => {
      if (closed) {
        return
      }
      closed = true
      logger.log("ipc.server_closing", {
        port,
        daemonUrl,
      })
      agentUpdateScheduler.close()
      await pluginSetup.close().catch(() => {})
      if (ownsConfigManager) {
        await configManager.close().catch(() => {})
      }
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error?: Error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
      logger.log("ipc.server_closed", {
        port,
        daemonUrl,
      })
      if (ownsStore) {
        store.close()
      }
    },
  }
}

async function setupDaemonPlugins(
  substrate: DaemonSetupSubstrate,
  configProvider: DaemonConfigProvider,
  backendClient: BackendClient,
  store: ComposedDaemonStore,
  plugins: readonly ComposedDaemonPlugin[],
  events: ComposedDaemonEvents,
) {
  const extensions: Record<string, unknown> = {}
  const extensionsByPluginName = new Map<string, Record<string, unknown>>()
  const ipcHandlers: Record<string, unknown> = {}
  const closeHandlers: Array<() => void | Promise<void>> = []

  for (const plugin of plugins) {
    const consumedExtensions = (plugin.consumes ?? []).map(
      (consumedPlugin) => extensionsByPluginName.get(consumedPlugin.name) ?? {},
    )
    const context = Object.assign(
      Object.create(substrate) as DaemonSetupSubstrate,
      {
        db: createPluginDbContext(plugin, store),
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

    if (setup?.close) {
      closeHandlers.push(setup.close)
    }

    if (setup?.provides) {
      Object.assign(extensions, setup.provides)
      extensionsByPluginName.set(plugin.name, setup.provides)
    }
  }

  return {
    extensions,
    ipcHandlers,
    close: async () => {
      for (let index = closeHandlers.length - 1; index >= 0; index -= 1) {
        await closeHandlers[index]?.()
      }
    },
  }
}

function observeDaemonEventsForLogging(events: ComposedDaemonEvents, logger: DaemonLogger) {
  const debugLoggers = new Map<string, DaemonDebugLogger>()

  events.observe((event) => {
    const fields = createDaemonEventLogFields(event)
    const debugScope = event.log?.debug
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

function createPluginDbContext(plugin: ComposedDaemonPlugin, store: ComposedDaemonStore) {
  const schema: Record<string, unknown> = {}
  for (const consumedPlugin of plugin.consumes ?? []) {
    Object.assign(schema, consumedPlugin.db?.schema)
  }
  Object.assign(schema, plugin.db?.schema)

  const contextSchema: Record<string, unknown> = {}
  const storeRecord = store as Record<string, unknown> & {
    readonly schema: Record<string, unknown>
  }
  const context: Record<string, unknown> = {
    schema: contextSchema,
    batch: (callback: () => unknown) => store.batch(callback),
  }

  for (const key of Object.keys(schema)) {
    contextSchema[key] = storeRecord.schema[key]
    Object.defineProperty(context, key, {
      enumerable: true,
      get: () => storeRecord[key],
    })
  }

  return context
}

function readBoundTcpPort(server: Server) {
  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("IPC server did not bind to a TCP port")
  }

  return address.port
}
