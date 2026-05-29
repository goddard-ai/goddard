import { randomUUID } from "node:crypto"
import { once } from "node:events"
import type { Server } from "node:http"
import { actionPlugin } from "@goddard-ai/action/daemon"
import { adapterPlugin } from "@goddard-ai/adapter/daemon"
import { authPlugin } from "@goddard-ai/auth/daemon"
import { composeBackendRoutes } from "@goddard-ai/backend-plugin"
import { composePlugins, type DaemonSetupSubstrate } from "@goddard-ai/daemon-plugin"
import { inboxPlugin } from "@goddard-ai/inbox/daemon"
import { $type, composeIpcRoutes, defineIpcRoutes, http } from "@goddard-ai/ipc"
import { createServer } from "@goddard-ai/ipc/node"
import { loopPlugin } from "@goddard-ai/loop/daemon"
import { pullRequestPlugin } from "@goddard-ai/pull-request/daemon"
import { reviewSessionPlugin } from "@goddard-ai/review-session/daemon"
import { type DaemonSession } from "@goddard-ai/schema/daemon"
import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { workforcePlugin } from "@goddard-ai/workforce/daemon"
import { createAcpRegistryService } from "acp-client/node"
import { getErrorMessage } from "radashi"

import type { BackendClient } from "../backend.ts"
import { createConfigManager } from "../config-manager.ts"
import { resolveRuntimeConfig } from "../config.ts"
import { IpcRequestContext, SetupContext } from "../context.ts"
import { createLogger, createPayloadPreview, readSessionIdForLog } from "../logging.ts"
import { configureDbSchema, db } from "../persistence/store.ts"
import type { DaemonServer } from "./types.ts"

type DaemonPluginComposition = ReturnType<typeof composePlugins>
type ComposedDaemonPlugin = DaemonPluginComposition["plugins"][number]

const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    health: http.get("health", {
      response: $type<{ ok: boolean }>(),
    }),
  }),
})

let daemonPlugins: DaemonPluginComposition | null = null

function getDaemonPlugins() {
  if (!daemonPlugins) {
    daemonPlugins = composePlugins([
      actionPlugin,
      adapterPlugin,
      authPlugin,
      sessionPlugin,
      inboxPlugin,
      pullRequestPlugin,
      reviewSessionPlugin,
      loopPlugin,
      workforcePlugin,
    ])
    configureDbSchema(daemonPlugins.db)
  }

  return daemonPlugins
}

/** Ensures daemon plugin composition has contributed schemas before tests reset the store. */
export function initializeDaemonPluginComposition() {
  getDaemonPlugins()
}

export async function startDaemonServer(
  client: BackendClient,
  options: {
    port?: number
    agentBinDir?: string
    idleSessionShutdownTimeoutMs?: number
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
    },
    authTokenStore: {
      set: (token) => {
        db.metadata.set("authToken", token)
      },
      delete: () => {
        db.metadata.delete("authToken")
      },
    },
    configManager,
    registryService,
    getIpcRequestContext: requireIpcRequestContext,
  } satisfies DaemonSetupSubstrate

  const pluginSetup = await setupDaemonPlugins(daemonSubstrate, client)
  const ipcHandlers = {
    daemon: {
      health: async () => ({ ok: true }),
    },
    ...pluginSetup.ipcHandlers,
  }

  const ipcServer = createServer({
    port: runtime.port,
    routes: composeIpcRoutes([coreDaemonIpcRoutes, getDaemonPlugins().ipcRoutes]),
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
    },
  }
}

async function setupDaemonPlugins(substrate: DaemonSetupSubstrate, backendClient: BackendClient) {
  const extensions: Record<string, unknown> = {}
  const extensionsByPluginName = new Map<string, Record<string, unknown>>()
  const ipcHandlers: Record<string, unknown> = {}
  const closeHandlers: Array<() => void | Promise<void>> = []

  for (const plugin of getDaemonPlugins().plugins) {
    const consumedExtensions = (plugin.consumes ?? []).map(
      (consumedPlugin) => extensionsByPluginName.get(consumedPlugin.name) ?? {},
    )
    const context = Object.assign(
      Object.create(substrate) as DaemonSetupSubstrate,
      {
        db: createPluginDbContext(plugin),
        backend: createPluginBackendContext(plugin, backendClient),
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

function createPluginDbContext(plugin: ComposedDaemonPlugin) {
  const schema: Record<string, unknown> = {}
  for (const consumedPlugin of plugin.consumes ?? []) {
    Object.assign(schema, consumedPlugin.db)
  }
  Object.assign(schema, plugin.db)

  const contextSchema: Record<string, unknown> = {}
  const context: Record<string, unknown> = {
    schema: contextSchema,
    batch: db.batch.bind(db),
  }

  for (const key of Object.keys(schema)) {
    context[key] = db[key]
    contextSchema[key] = db.schema[key]
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
