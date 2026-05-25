import { randomUUID } from "node:crypto"
import { once } from "node:events"
import type { Server } from "node:http"
import { actionPlugin } from "@goddard-ai/action/daemon"
import { adapterPlugin } from "@goddard-ai/adapter/daemon"
import { authPlugin } from "@goddard-ai/auth/daemon"
import { composeBackendRoutes } from "@goddard-ai/backend-plugin"
import { composePlugins, type DaemonSetupSubstrate } from "@goddard-ai/daemon-plugin"
import { inboxPlugin } from "@goddard-ai/inbox/daemon"
import { getResponsePluginMarkerId } from "@goddard-ai/ipc"
import { createServer } from "@goddard-ai/ipc/node"
import { loopPlugin } from "@goddard-ai/loop/daemon"
import { pullRequestPlugin } from "@goddard-ai/pull-request/daemon"
import { type DaemonSession } from "@goddard-ai/schema/daemon"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { workforcePlugin } from "@goddard-ai/workforce/daemon"
import { getErrorMessage } from "radashi"

import type { BackendClient } from "../backend.ts"
import { createConfigManager } from "../config-manager.ts"
import { resolveRuntimeConfig } from "../config.ts"
import { IpcRequestContext, SetupContext } from "../context.ts"
import { createLogger, createPayloadPreview, readSessionIdForLog } from "../logging.ts"
import { configureDbSchema, db } from "../persistence/store.ts"
import { createACPRegistryService } from "../session/registry.ts"
import type { DaemonServer } from "./types.ts"

const daemonPlugins = composePlugins([
  actionPlugin,
  adapterPlugin,
  authPlugin,
  sessionPlugin,
  inboxPlugin,
  pullRequestPlugin,
  loopPlugin,
  workforcePlugin,
])
configureDbSchema(daemonPlugins.db)

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

  const registryService = createACPRegistryService()
  let publishPluginEvent: ((name: string, payload: unknown) => void) | null = null
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

  const pluginSetup = await setupDaemonPlugins(daemonSubstrate, client, (plugin, name, payload) => {
    if (!plugin.ipcRoutes || !hasIpcStreamRouteName(plugin.ipcRoutes, name.split("."))) {
      throw new Error(`Daemon plugin ${plugin.name} cannot publish undeclared IPC stream ${name}`)
    }
    if (!publishPluginEvent) {
      throw new Error(`Daemon plugin ${plugin.name} published IPC stream ${name} before startup`)
    }
    publishPluginEvent(name, payload)
  })
  const requestHandlersFromPlugins = filterPluginIpcHandlers(pluginSetup.ipcHandlers, "request")
  const streamHandlersFromPlugins = filterPluginIpcHandlers(pluginSetup.ipcHandlers, "stream")
  const streamLifecycleFromPlugins = pluginSetup.ipcStreamLifecycle

  const requestHandlers: Record<string, (payload: any) => any> = {
    "daemon.health": async () => ({ ok: true }),
    ...requestHandlersFromPlugins,
  }

  const ipcServer = createServer({
    port: runtime.port,
    schema: daemonIpcSchema as any,
    handlers: requestHandlers as any,
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
    beforeSubscribe: async ({ name, filter }) => {
      await runPluginStreamHandler(streamHandlersFromPlugins[name], filter)
    },
    afterSubscribe: async ({ name, filter }) => {
      await runPluginStreamLifecycleHook(streamLifecycleFromPlugins[name]?.afterSubscribe, filter)
    },
    afterUnsubscribe: async ({ name, filter }) => {
      await runPluginStreamLifecycleHook(streamLifecycleFromPlugins[name]?.afterUnsubscribe, filter)
    },
  })

  publishPluginEvent = (name, payload) => {
    ipcServer.publish(name as never, payload as never)
  }

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

async function setupDaemonPlugins(
  substrate: DaemonSetupSubstrate,
  backendClient: BackendClient,
  publish: (plugin: (typeof daemonPlugins.plugins)[number], name: string, payload: unknown) => void,
) {
  const extensions: Record<string, unknown> = {}
  const extensionsByPluginName = new Map<string, Record<string, unknown>>()
  const ipcHandlers: Record<string, unknown> = {}
  const ipcStreamLifecycle: Record<string, IpcStreamLifecycleHandlers> = {}
  const closeHandlers: Array<() => void | Promise<void>> = []

  for (const plugin of daemonPlugins.plugins) {
    const consumedExtensions = (plugin.consumes ?? []).map(
      (consumedPlugin) => extensionsByPluginName.get(consumedPlugin.name) ?? {},
    )
    const context = Object.assign(
      Object.create(substrate) as DaemonSetupSubstrate,
      {
        db: createPluginDbContext(plugin),
        backend: createPluginBackendContext(plugin, backendClient),
        publish: (name: string, payload: unknown) => {
          publish(plugin, name, payload)
        },
      },
      ...consumedExtensions,
    )
    const setup = await plugin.setup?.(context)

    if (setup?.ipcHandlers) {
      Object.assign(ipcHandlers, flattenIpcHandlers(setup.ipcHandlers))
    }

    if (setup?.ipcStreamLifecycle) {
      Object.assign(ipcStreamLifecycle, flattenIpcStreamLifecycle(setup.ipcStreamLifecycle))
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
    ipcStreamLifecycle,
    close: async () => {
      for (let index = closeHandlers.length - 1; index >= 0; index -= 1) {
        await closeHandlers[index]?.()
      }
    },
  }
}

type IpcStreamLifecycleHandlers = {
  readonly afterSubscribe?: (filter: unknown) => void | Promise<void>
  readonly afterUnsubscribe?: (filter: unknown) => void | Promise<void>
}

function flattenIpcHandlers(ipcHandlers: Record<string, unknown>) {
  const requestHandlers: Record<string, unknown> = {}

  function visit(node: unknown, path: string[]) {
    if (typeof node === "function") {
      requestHandlers[path.join(".")] = (payload: unknown) =>
        node({ body: payload, query: payload })
      return
    }

    if (typeof node !== "object" || !node) {
      return
    }

    for (const [key, child] of Object.entries(node)) {
      visit(child, [...path, key])
    }
  }

  visit(ipcHandlers, [])
  return requestHandlers
}

function flattenIpcStreamLifecycle(lifecycle: Record<string, unknown>) {
  const handlers: Record<string, IpcStreamLifecycleHandlers> = {}

  function visit(node: unknown, path: string[]) {
    if (isIpcStreamLifecycleHandlers(node)) {
      handlers[path.join(".")] = node
      return
    }

    if (typeof node !== "object" || !node) {
      return
    }

    for (const [key, child] of Object.entries(node)) {
      visit(child, [...path, key])
    }
  }

  visit(lifecycle, [])
  return handlers
}

function isIpcStreamLifecycleHandlers(value: unknown): value is IpcStreamLifecycleHandlers {
  return (
    typeof value === "object" &&
    value !== null &&
    ("afterSubscribe" in value || "afterUnsubscribe" in value)
  )
}

function filterPluginIpcHandlers(ipcHandlers: Record<string, unknown>, kind: "request" | "stream") {
  const routeNames = kind === "request" ? daemonIpcSchema.requests : daemonIpcSchema.streams
  return Object.fromEntries(
    Object.entries(ipcHandlers).filter(([name]) => name in routeNames),
  ) as Record<string, (payload: any) => any>
}

async function runPluginStreamHandler(
  handler: ((payload: unknown) => unknown) | undefined,
  filter: unknown,
) {
  if (!handler) {
    return
  }

  const result = handler(filter)
  if (isAsyncIterator(result)) {
    await result.next()
    return
  }

  await result
}

async function runPluginStreamLifecycleHook(
  handler: ((filter: unknown) => void | Promise<void>) | undefined,
  filter: unknown,
) {
  if (!handler) {
    return
  }

  await handler(filter)
}

function isAsyncIterator(value: unknown): value is AsyncIterator<unknown> {
  return typeof value === "object" && value !== null && "next" in value
}

function hasIpcStreamRouteName(routes: Record<string, unknown>, path: readonly string[]) {
  let current: unknown = routes

  for (const segment of path) {
    if (typeof current !== "object" || !current || !(segment in current)) {
      return false
    }
    const node = current[segment as keyof typeof current]
    current =
      typeof node === "object" && node && "children" in node
        ? ((node as { readonly children: Record<string, unknown> }).children as Record<
            string,
            unknown
          >)
        : node
  }

  return isIpcStreamRouteNode(current)
}

function isIpcStreamRouteNode(value: unknown) {
  if (
    typeof value !== "object" ||
    value === null ||
    !("kind" in value) ||
    value.kind !== "action"
  ) {
    return false
  }

  const response = (value as { readonly schema?: { readonly response?: unknown } }).schema?.response
  return getResponsePluginMarkerId(response) === "rouzer/ndjson"
}

function createPluginBackendContext(
  plugin: (typeof daemonPlugins.plugins)[number],
  client: BackendClient,
) {
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

function createPluginDbContext(plugin: (typeof daemonPlugins.plugins)[number]) {
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
