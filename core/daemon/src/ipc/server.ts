import { randomUUID } from "node:crypto"
import { once } from "node:events"
import type { Server } from "node:http"
import { composeBackendRoutes } from "@goddard-ai/backend-plugin"
import { type DaemonConfigProvider, type DaemonSetupSubstrate } from "@goddard-ai/daemon-plugin"
import { $type, composeIpcRoutes, defineIpcRoutes, http } from "@goddard-ai/ipc"
import { createServer } from "@goddard-ai/ipc/node"
import { type DaemonSession } from "@goddard-ai/schema/daemon"
import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { createAcpRegistryService } from "acp-client/node"
import { getErrorMessage } from "radashi"

import type { BackendClient } from "../backend.ts"
import { createConfigManager } from "../config-manager.ts"
import { prependAgentBinToPath, resolveRuntimeConfig } from "../config.ts"
import { IpcRequestContext, SessionContext, SetupContext } from "../context.ts"
import {
  createChunkPreview,
  createLogger,
  createPayloadPreview,
  readSessionIdForLog,
} from "../logging.ts"
import { openDaemonStore, type DaemonStore } from "../persistence/store.ts"
import { getDaemonPluginComposition } from "../plugins.ts"
import type { DaemonServer } from "./types.ts"

type ComposedDaemonPlugin = ReturnType<typeof getDaemonPluginComposition>["plugins"][number]

const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    health: http.get("health", {
      response: $type<{ ok: boolean }>(),
    }),
  }),
})

/** Ensures daemon plugin composition has contributed schemas before tests reset the store. */
export function initializeDaemonPluginComposition() {
  getDaemonPluginComposition()
}

export async function startDaemonServer(
  client: BackendClient,
  options: {
    port?: number
    agentBinDir?: string
    idleSessionShutdownTimeoutMs?: number
    store?: DaemonStore
  } = {},
): Promise<DaemonServer> {
  const logger = createLogger()
  const setupContext = SetupContext.get()
  const runtime =
    setupContext?.runtime ??
    resolveRuntimeConfig({
      port: options.port,
      agentBinDir: options.agentBinDir,
    })
  const configManager = setupContext?.configManager ?? createConfigManager()
  const ownsConfigManager = setupContext == null
  const store = options.store ?? openDaemonStore()
  const ownsStore = options.store == null

  const registryService = createAcpRegistryService()
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
      createLogger,
      createPayloadPreview,
      createChunkPreview,
    },
    registryService,
    sessionContext: {
      run: (context, callback) => SessionContext.run(context, callback),
    },
    getIpcRequestContext: requireIpcRequestContext,
  } satisfies DaemonSetupSubstrate

  const pluginSetup = await setupDaemonPlugins(
    daemonSubstrate,
    {
      getRootConfig: configManager.getRootConfig,
      getLastKnownRootConfig: configManager.getLastKnownRootConfig,
    },
    client,
    store,
  )
  const ipcHandlers = {
    daemon: {
      health: async () => ({ ok: true }),
    },
    ...pluginSetup.ipcHandlers,
  }

  const ipcServer = createServer({
    port: runtime.port,
    routes: composeIpcRoutes([coreDaemonIpcRoutes, getDaemonPluginComposition().ipcRoutes]),
    handlers: ipcHandlers as any,
    runHandler: ({ payload }, handler) => {
      const context: IpcRequestContext = {
        opId: randomUUID(),
        sessionId: readSessionIdForLog(payload) ?? null,
        setSessionId(sessionId: DaemonSession["id"]) {
          context.sessionId = sessionId
        },
      }
      return IpcRequestContext.run(context, handler)
    },
    onRequestReceived: ({ name, payload }) => {
      logger.log("ipc.request_received", {
        requestName: name,
        payload: createPayloadPreview(payload),
      })
    },
    onResponseSent: ({ name, response, durationMs }) => {
      const responseSessionId = readSessionIdForLog(response)
      if (responseSessionId) {
        const context = requireIpcRequestContext()
        context.setSessionId(responseSessionId)
      }

      logger.log("ipc.response_sent", {
        requestName: name,
        durationMs,
        response: createPayloadPreview(response),
      })
    },
    onRequestFailed: ({ name, error, durationMs }) => {
      logger.log("ipc.request_failed", {
        requestName: name,
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

  let closed = false

  return {
    daemonUrl,
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
  store: DaemonStore,
) {
  const extensions: Record<string, unknown> = {}
  const extensionsByPluginName = new Map<string, Record<string, unknown>>()
  const ipcHandlers: Record<string, unknown> = {}
  const closeHandlers: Array<() => void | Promise<void>> = []

  for (const plugin of getDaemonPluginComposition().plugins) {
    const consumedExtensions = (plugin.consumes ?? []).map(
      (consumedPlugin) => extensionsByPluginName.get(consumedPlugin.name) ?? {},
    )
    const context = Object.assign(
      Object.create(substrate) as DaemonSetupSubstrate,
      {
        db: createPluginDbContext(plugin, store),
        backend: createPluginBackendContext(plugin, backendClient),
        configProvider: createPluginConfigProvider(configProvider, plugin),
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

function createPluginDbContext(plugin: ComposedDaemonPlugin, store: DaemonStore) {
  const schema: Record<string, unknown> = {}
  for (const consumedPlugin of plugin.consumes ?? []) {
    Object.assign(schema, consumedPlugin.db)
  }
  Object.assign(schema, plugin.db)

  const contextSchema: Record<string, unknown> = {}
  const context: Record<string, unknown> = {
    schema: contextSchema,
    batch: (...args: unknown[]) => store.batch(...(args as never[])),
  }

  for (const key of Object.keys(schema)) {
    contextSchema[key] = store.schema[key]
    Object.defineProperty(context, key, {
      enumerable: true,
      get: () => store[key],
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
