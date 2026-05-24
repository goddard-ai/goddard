import { randomUUID } from "node:crypto"
import { once } from "node:events"
import type { Server } from "node:http"
import { actionPlugin } from "@goddard-ai/action/daemon"
import { adapterPlugin } from "@goddard-ai/adapter/daemon"
import { authPlugin } from "@goddard-ai/auth/daemon"
import { composeBackendRoutes } from "@goddard-ai/backend-plugin"
import {
  composePlugins,
  type DaemonSetupSubstrate,
  type InferProvides,
} from "@goddard-ai/daemon-plugin"
import { inboxPlugin } from "@goddard-ai/inbox/daemon"
import { createServer, IpcClientError } from "@goddard-ai/ipc/node"
import { loopPlugin } from "@goddard-ai/loop/daemon"
import { pullRequestPlugin } from "@goddard-ai/pull-request/daemon"
import { type DaemonSession, type SubscribeWorkforceEventsRequest } from "@goddard-ai/schema/daemon"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import {
  createSessionManager,
  sessionPlugin,
  type SessionManager,
} from "@goddard-ai/session/daemon"
import { getErrorMessage } from "radashi"

import type { BackendClient } from "../backend.ts"
import { createConfigManager } from "../config-manager.ts"
import { resolveRuntimeConfig } from "../config.ts"
import { IpcRequestContext, SetupContext, type WorkforceActorContext } from "../context.ts"
import { createLogger, createPayloadPreview, readSessionIdForLog } from "../logging.ts"
import { configureDbSchema, db } from "../persistence/store.ts"
import { createACPRegistryService } from "../session/registry.ts"
import {
  discoverWorkforceInitCandidates,
  initializeWorkforce,
  resolveRepositoryRoot,
} from "../workforce/config.ts"
import { createWorkforceManager, type WorkforceManager } from "../workforce/index.ts"
import { normalizeWorkforceRootDir } from "../workforce/paths.ts"
import type { DaemonServer } from "./types.ts"

const daemonPlugins = composePlugins([
  actionPlugin,
  adapterPlugin,
  authPlugin,
  sessionPlugin,
  inboxPlugin,
  pullRequestPlugin,
  loopPlugin,
])
configureDbSchema(daemonPlugins.db)

type SessionExtension = InferProvides<typeof sessionPlugin>["session"]

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

  let sessionManager!: SessionManager
  let workforceManager!: WorkforceManager

  function requireIpcRequestContext() {
    const context = IpcRequestContext.get()
    if (!context) {
      throw new Error("IPC request context is unavailable")
    }

    return context
  }

  async function resolveWorkforceActor(
    token: string | undefined,
    requestedRootDir: string,
  ): Promise<WorkforceActorContext> {
    if (!token) {
      return {
        sessionId: null,
        rootDir: null,
        agentId: null,
        requestId: null,
      }
    }

    const session = await sessionFeature.resolveTokenScope(token)
    if (!session) {
      throw new IpcClientError("Invalid session token")
    }

    const context = requireIpcRequestContext()
    context.setSessionId(session.sessionId)

    const workforceRecord =
      db.workforces.first({
        where: { sessionId: session.sessionId },
      }) ?? null

    if (!workforceRecord || typeof workforceRecord.agentId !== "string") {
      throw new IpcClientError("Session is not attached to a workforce request")
    }

    if (typeof workforceRecord.rootDir !== "string") {
      throw new IpcClientError("Session is not attached to a workforce root")
    }

    const [sessionRootDir, normalizedRequestedRootDir] = await Promise.all([
      normalizeWorkforceRootDir(workforceRecord.rootDir),
      normalizeWorkforceRootDir(requestedRootDir),
    ])

    if (sessionRootDir !== normalizedRequestedRootDir) {
      throw new IpcClientError(
        `Session workforce root ${sessionRootDir} does not match requested root ${normalizedRequestedRootDir}`,
      )
    }

    return {
      sessionId: session.sessionId,
      rootDir: sessionRootDir,
      agentId: workforceRecord.agentId,
      requestId: typeof workforceRecord.requestId === "string" ? workforceRecord.requestId : null,
    }
  }

  function requireActorRequestId(actor: WorkforceActorContext): string {
    if (!actor.requestId) {
      throw new IpcClientError("Session is not attached to an active workforce request")
    }

    return actor.requestId
  }

  const daemonSubstrate = {
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
    get sessionManager() {
      if (!sessionManager) {
        throw new Error("Session manager is unavailable before daemon startup completes")
      }
      return sessionManager
    },
    getIpcRequestContext: requireIpcRequestContext,
  } satisfies DaemonSetupSubstrate

  const pluginSetup = await setupDaemonPlugins(daemonSubstrate, client, (plugin, name, payload) => {
    if (!plugin.ipcRoutes || !hasIpcRouteName(plugin.ipcRoutes, name.split("."))) {
      throw new Error(`Daemon plugin ${plugin.name} cannot publish undeclared IPC route ${name}`)
    }
    if (!publishPluginEvent) {
      throw new Error(`Daemon plugin ${plugin.name} published IPC route ${name} before startup`)
    }
    publishPluginEvent(name, payload)
  })
  const sessionFeature = pluginSetup.extensions.session as SessionExtension

  const requestHandlers: Record<string, (payload: any) => any> = {
    "daemon.health": async () => ({ ok: true }),
    ...pluginSetup.requestHandlers,
    "workforce.start": async ({ rootDir }: any) => {
      return {
        workforce: await workforceManager.startWorkforce(rootDir),
      }
    },
    "workforce.discoverCandidates": async ({ rootDir }: any) => {
      // Canonicalize the repo root inside the daemon so SDK and CLI callers cannot drift.
      const repositoryRoot = await resolveRepositoryRoot(rootDir)
      return {
        rootDir: repositoryRoot,
        candidates: await discoverWorkforceInitCandidates(repositoryRoot),
      }
    },
    "workforce.initialize": async ({ rootDir, packageDirs }: any) => {
      // Re-resolve here for the same reason as discovery: the daemon owns the canonical root.
      const repositoryRoot = await resolveRepositoryRoot(rootDir)
      return {
        initialized: await initializeWorkforce(repositoryRoot, packageDirs),
      }
    },
    "workforce.get": async ({ rootDir }: any) => {
      return {
        workforce: await workforceManager.getWorkforce(rootDir),
      }
    },
    "workforce.list": async () => {
      return {
        workforces: await workforceManager.listWorkforces(),
      }
    },
    "workforce.shutdown": async ({ rootDir }: any) => {
      return {
        rootDir,
        success: await workforceManager.shutdownWorkforce(rootDir),
      }
    },
    "workforce.request": async (payload: any) => {
      const actor = await resolveWorkforceActor(payload.token, payload.rootDir)
      return workforceManager.appendWorkforceEvent(
        actor.rootDir ?? payload.rootDir,
        {
          type: "request",
          targetAgentId: payload.targetAgentId,
          input: payload.input,
          intent: payload.intent,
        },
        actor,
      )
    },
    "workforce.update": async (payload: any) => {
      const actor = await resolveWorkforceActor(payload.token, payload.rootDir)
      return workforceManager.appendWorkforceEvent(
        actor.rootDir ?? payload.rootDir,
        {
          type: "update",
          requestId: payload.requestId,
          input: payload.input,
        },
        actor,
      )
    },
    "workforce.cancel": async (payload: any) => {
      const actor = await resolveWorkforceActor(payload.token, payload.rootDir)
      return workforceManager.appendWorkforceEvent(
        actor.rootDir ?? payload.rootDir,
        {
          type: "cancel",
          requestId: payload.requestId,
          reason: payload.reason ?? null,
        },
        actor,
      )
    },
    "workforce.truncate": async (payload: any) => {
      const actor = await resolveWorkforceActor(payload.token, payload.rootDir)
      return workforceManager.appendWorkforceEvent(
        actor.rootDir ?? payload.rootDir,
        {
          type: "truncate",
          agentId: payload.agentId ?? null,
          reason: payload.reason ?? null,
        },
        actor,
      )
    },
    "workforce.respond": async (payload: any) => {
      const actor = await resolveWorkforceActor(payload.token, payload.rootDir)
      return workforceManager.appendWorkforceEvent(
        actor.rootDir ?? payload.rootDir,
        {
          type: "respond",
          requestId: requireActorRequestId(actor),
          output: payload.output,
        },
        actor,
      )
    },
    "workforce.suspend": async (payload: any) => {
      const actor = await resolveWorkforceActor(payload.token, payload.rootDir)
      return workforceManager.appendWorkforceEvent(
        actor.rootDir ?? payload.rootDir,
        {
          type: "suspend",
          requestId: requireActorRequestId(actor),
          reason: payload.reason,
        },
        actor,
      )
    },
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
      if (name === "workforce.event") {
        const request = filter as SubscribeWorkforceEventsRequest | undefined
        if (!request) {
          throw new IpcClientError("Missing workforce event filter")
        }

        // Subscription setup only validates that this workforce is active; events are
        // still pushed later from the runtime when new ledger activity is appended.
        await workforceManager.getWorkforce(request.rootDir)
      }
    },
    afterSubscribe: async ({ name, filter }) => {
      if (name === "session.message") {
        const streamFilter = filter as any
        const sessionId =
          typeof streamFilter === "object" && streamFilter && "id" in streamFilter
            ? streamFilter.id
            : null
        if (typeof sessionId === "string") {
          await sessionFeature.subscriberConnected(sessionId as `ses_${string}`)
        }
      }
    },
    afterUnsubscribe: async ({ name, filter }) => {
      if (name === "session.message") {
        const streamFilter = filter as any
        const sessionId =
          typeof streamFilter === "object" && streamFilter && "id" in streamFilter
            ? streamFilter.id
            : null
        if (typeof sessionId === "string") {
          await sessionFeature.subscriberDisconnected(sessionId as `ses_${string}`)
        }
      }
    },
  })

  publishPluginEvent = (name, payload) => {
    ipcServer.publish(name as never, payload as never)
  }

  await once(ipcServer.server, "listening")
  const port = readBoundTcpPort(ipcServer.server)
  const daemonUrl = createDaemonUrl(port)

  sessionManager = createSessionManager({
    daemonUrl,
    agentBinDir: runtime.agentBinDir,
    configManager,
    registryService,
    events: sessionFeature.events,
    idleSessionShutdownTimeoutMs: options.idleSessionShutdownTimeoutMs,
    publish(id, message) {
      ipcServer.publish("session.message", { id, message })
    },
  })

  workforceManager = createWorkforceManager({
    sessionManager,
    publishEvent(payload) {
      ipcServer.publish("workforce.event", payload)
    },
  })
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
      await workforceManager.close().catch(() => {})
      await sessionManager.close().catch(() => {})
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
  const requestHandlers: Record<string, unknown> = {}
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
      Object.assign(requestHandlers, flattenIpcHandlers(setup.ipcHandlers))
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
    requestHandlers,
    close: async () => {
      for (let index = closeHandlers.length - 1; index >= 0; index -= 1) {
        await closeHandlers[index]?.()
      }
    },
  }
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

function hasIpcRouteName(routes: Record<string, unknown>, path: readonly string[]) {
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

  return typeof current === "object" && current != null && "kind" in current
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
