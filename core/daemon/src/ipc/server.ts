import { randomUUID } from "node:crypto"
import { once } from "node:events"
import type { Server } from "node:http"
import { adapterPlugin } from "@goddard-ai/adapter/daemon"
import { inboxPlugin } from "@goddard-ai/inbox/daemon"
import type { Handlers } from "@goddard-ai/ipc"
import { createServer, IpcClientError } from "@goddard-ai/ipc/node"
import { type DaemonSession, type SubscribeWorkforceEventsRequest } from "@goddard-ai/schema/daemon"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { sessionPlugin, type SessionController } from "@goddard-ai/session/daemon"
import { getErrorMessage } from "radashi"

import { createConfigManager } from "../config-manager.ts"
import { resolveRuntimeConfig } from "../config.ts"
import { IpcRequestContext, SetupContext, type WorkforceActorContext } from "../context.ts"
import { createInboxManager } from "../inbox/manager.ts"
import { createLogger, createPayloadPreview, readSessionIdForLog } from "../logging.ts"
import { createLoopManager, type LoopManager } from "../loop/index.ts"
import { db } from "../persistence/store.ts"
import { buildNamedActionSessionParams, resolveNamedAction } from "../resolvers/actions.ts"
import { resolveNamedLoopStartRequest } from "../resolvers/loops.ts"
import { createSessionManager, type SessionManager } from "../session/manager.ts"
import { createACPRegistryService } from "../session/registry.ts"
import {
  discoverWorkforceInitCandidates,
  initializeWorkforce,
  resolveRepositoryRoot,
} from "../workforce/config.ts"
import { createWorkforceManager, type WorkforceManager } from "../workforce/index.ts"
import { normalizeWorkforceRootDir } from "../workforce/paths.ts"
import { resolveReplyRequestFromGit, resolveSubmitRequestFromGit } from "./git.ts"
import type { BackendPrClient, DaemonServer } from "./types.ts"

export async function startDaemonServer(
  client: BackendPrClient,
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
  let publishInboxItemEvent: Parameters<typeof createInboxManager>[0]["publishEvent"] = () => {}
  const inboxManager = createInboxManager({
    publishEvent: (payload) => {
      publishInboxItemEvent(payload)
    },
  })

  let sessionManager!: SessionManager
  let loopManager!: LoopManager
  let workforceManager!: WorkforceManager

  async function getSessionByToken(token: string) {
    const sessionRecord =
      db.sessions.first({
        where: { token },
      }) ?? null
    if (!sessionRecord?.permissions) {
      return null
    }

    return {
      sessionId: sessionRecord.id,
      owner: sessionRecord.permissions.owner,
      repo: sessionRecord.permissions.repo,
      allowedPrNumbers: sessionRecord.permissions.allowedPrNumbers,
    }
  }

  async function addAllowedPrToSession(sessionId: DaemonSession["id"], prNumber: number) {
    const sessionRecord = db.sessions.get(sessionId)
    if (!sessionRecord?.permissions) {
      return
    }

    db.sessions.update(sessionId, (record) => {
      if (!record.permissions || record.permissions.allowedPrNumbers.includes(prNumber)) {
        return record
      }

      return {
        ...record,
        permissions: {
          ...record.permissions,
          allowedPrNumbers: [...record.permissions.allowedPrNumbers, prNumber],
        },
      }
    })
  }

  async function recordPullRequest(record: Parameters<typeof db.pullRequests.create>[0]) {
    return db.pullRequests.putByUnique(
      {
        host: record.host,
        owner: record.owner,
        repo: record.repo,
        prNumber: record.prNumber,
      },
      record,
    )
  }

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

    const session = await getSessionByToken(token)
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

  const sessionController: SessionController = {
    newSession: (params) => sessionManager.newSession(params),
    listSessions: (params) => sessionManager.listSessions(params),
    connectSession: (id) => sessionManager.connectSession(id),
    getSession: (id) => sessionManager.getSession(id),
    getHistory: (params) => sessionManager.getHistory(params),
    getChanges: (id) => sessionManager.getChanges(id),
    getComposerSuggestions: (params) => sessionManager.getComposerSuggestions(params),
    getDraftSuggestions: (params) => sessionManager.getDraftSuggestions(params),
    getLaunchPreview: (params) => sessionManager.getLaunchPreview(params),
    getSubpackages: (params) => sessionManager.getSubpackages(params),
    getDiagnostics: (id) => sessionManager.getDiagnostics(id),
    getWorktree: (id) => sessionManager.getWorktree(id),
    mountReviewSession: (id) => sessionManager.mountReviewSession(id),
    runReviewSession: (id) => sessionManager.runReviewSession(id),
    unmountReviewSession: (id) => sessionManager.unmountReviewSession(id),
    getWorkforce: (id) => sessionManager.getWorkforce(id),
    shutdownSession: (id) => sessionManager.shutdownSession(id),
    cancelSessionTurn: (id) => sessionManager.cancelSessionTurn(id),
    steerSession: (id, prompt) => sessionManager.steerSession(id, prompt),
    sendMessage: (id, message) => sessionManager.sendMessage(id, message),
    completeSession: (id) => sessionManager.completeSession(id),
    declareInitiative: (id, title) => sessionManager.declareInitiative(id, title),
    reportBlocker: (id, reason, metadata) => sessionManager.reportBlocker(id, reason, metadata),
    reportTurnEnded: (id, metadata) => sessionManager.reportTurnEnded(id, metadata),
    recordTurnAttentionActivity: (id, metadata) =>
      sessionManager.recordTurnAttentionActivity(id, metadata),
    sessionSubscriberConnected: (id) => sessionManager.sessionSubscriberConnected(id),
    sessionSubscriberDisconnected: (id) => sessionManager.sessionSubscriberDisconnected(id),
    resolveSessionIdByToken: (token) => sessionManager.resolveSessionIdByToken(token),
  }

  const adapterSetup = await adapterPlugin.setup?.({ registryService, configManager })
  const inboxSetup = await inboxPlugin.setup?.({ inboxManager })
  const sessionSetup = await sessionPlugin.setup?.({
    controller: sessionController,
    setRequestSessionId: (id) => {
      requireIpcRequestContext().setSessionId(id)
    },
  })

  if (!adapterSetup?.requestHandlers) {
    throw new Error("Adapter daemon plugin did not return request handlers")
  }
  if (!inboxSetup?.requestHandlers) {
    throw new Error("Inbox daemon plugin did not return request handlers")
  }
  if (!sessionSetup?.requestHandlers || !sessionSetup.provides) {
    throw new Error("Session daemon plugin did not return request handlers")
  }
  const sessionFeature = sessionSetup.provides.session

  const requestHandlers = {
    "daemon.health": async () => ({ ok: true }),
    ...adapterSetup.requestHandlers,
    "auth.device.start": async (payload) => client.auth.startDeviceFlow(payload),
    "auth.device.complete": async (payload) => {
      const session = await client.auth.completeDeviceFlow(payload)
      db.metadata.set("authToken", session.token)
      return session
    },
    "auth.whoami": async () => client.auth.whoami(),
    "auth.logout": async () => {
      await client.auth.logout()
      db.metadata.delete("authToken")
      return { success: true as const }
    },
    "pr.submit": async (payload) => {
      const session = await getSessionByToken(payload.token)
      if (!session) {
        throw new IpcClientError("Invalid session token")
      }
      const context = requireIpcRequestContext()
      context.setSessionId(session.sessionId)
      if (!session.owner || !session.repo) {
        throw new IpcClientError("Session is not scoped to a repository")
      }

      const resolvedInput = await resolveSubmitRequestFromGit({
        cwd: payload.cwd,
        title: payload.title,
        body: payload.body,
        head: payload.head,
        base: payload.base,
      })

      const pr = await client.pr.create({
        ...resolvedInput,
        owner: session.owner,
        repo: session.repo,
      })
      await addAllowedPrToSession(session.sessionId, pr.number)
      const pullRequest = await recordPullRequest({
        host: "github",
        owner: session.owner,
        repo: session.repo,
        prNumber: pr.number,
        cwd: payload.cwd,
      })
      const metadata = await sessionFeature.recordTurnAttentionActivity(session.sessionId, {
        scope: payload.scope,
        headline: payload.headline,
        fallbackHeadline: resolvedInput.title,
      })
      inboxManager.touchInboxItem({
        entityId: pullRequest.id,
        reason: "pull_request.created",
        scope: metadata.scope,
        headline: metadata.headline,
        turnId: metadata.turnId,
      })
      db.sessions.update(session.sessionId, {
        status: "done",
        lastAgentMessage: `PR Submitted: ${resolvedInput.title}\n${pr.url}\n\n${
          resolvedInput.body ?? ""
        }`,
      })
      return { number: pr.number, url: pr.url }
    },
    "pr.get": async ({ id }) => {
      return {
        pullRequest: inboxManager.getPullRequest(id),
      }
    },
    "pr.reply": async (payload) => {
      const session = await getSessionByToken(payload.token)
      if (!session) {
        throw new IpcClientError("Invalid session token")
      }
      const context = requireIpcRequestContext()
      context.setSessionId(session.sessionId)
      if (!session.owner || !session.repo) {
        throw new IpcClientError("Session is not scoped to a repository")
      }

      const resolvedInput = await resolveReplyRequestFromGit({
        cwd: payload.cwd,
        message: payload.message,
        prNumber: payload.prNumber,
      })

      if (!session.allowedPrNumbers.includes(resolvedInput.prNumber)) {
        throw new IpcClientError(`PR #${resolvedInput.prNumber} is not allowed for this session`)
      }

      const response = await client.pr.reply({
        ...resolvedInput,
        owner: session.owner,
        repo: session.repo,
      })
      const pullRequest = await recordPullRequest({
        host: "github",
        owner: session.owner,
        repo: session.repo,
        prNumber: resolvedInput.prNumber,
        cwd: payload.cwd,
      })
      const metadata = await sessionFeature.recordTurnAttentionActivity(session.sessionId, {
        scope: payload.scope,
        headline: payload.headline,
        fallbackHeadline: "PR reply posted",
      })
      inboxManager.touchInboxItem({
        entityId: pullRequest.id,
        reason: "pull_request.updated",
        scope: metadata.scope,
        headline: metadata.headline,
        turnId: metadata.turnId,
      })
      db.sessions.update(session.sessionId, {
        status: "done",
        lastAgentMessage: `PR Reply: ${payload.message}`,
      })
      return response
    },
    ...sessionSetup.requestHandlers,
    ...inboxSetup.requestHandlers,
    "action.run": async (payload) => {
      const action = await resolveNamedAction(payload.actionName, payload.cwd, configManager)
      const session = await sessionFeature.create(
        buildNamedActionSessionParams(action, payload.cwd, {
          cwd: payload.cwd,
          agent: payload.agent,
          mcpServers: payload.mcpServers,
          env: payload.env,
          systemPrompt: payload.systemPrompt,
          repository: payload.repository,
          prNumber: payload.prNumber,
          metadata: payload.metadata,
        }),
      )
      const context = requireIpcRequestContext()
      context.setSessionId(session.id)
      return { session }
    },
    "loop.start": async (payload) => {
      return {
        loop: await loopManager.startLoop(payload),
      }
    },
    "loop.get": async ({ rootDir, loopName }) => {
      return {
        loop: await loopManager.getLoop(rootDir, loopName),
      }
    },
    "loop.list": async () => {
      return {
        loops: await loopManager.listLoops(),
      }
    },
    "loop.shutdown": async ({ rootDir, loopName }) => {
      return {
        rootDir,
        loopName,
        success: await loopManager.shutdownLoop(rootDir, loopName),
      }
    },
    "workforce.start": async ({ rootDir }) => {
      return {
        workforce: await workforceManager.startWorkforce(rootDir),
      }
    },
    "workforce.discoverCandidates": async ({ rootDir }) => {
      // Canonicalize the repo root inside the daemon so SDK and CLI callers cannot drift.
      const repositoryRoot = await resolveRepositoryRoot(rootDir)
      return {
        rootDir: repositoryRoot,
        candidates: await discoverWorkforceInitCandidates(repositoryRoot),
      }
    },
    "workforce.initialize": async ({ rootDir, packageDirs }) => {
      // Re-resolve here for the same reason as discovery: the daemon owns the canonical root.
      const repositoryRoot = await resolveRepositoryRoot(rootDir)
      return {
        initialized: await initializeWorkforce(repositoryRoot, packageDirs),
      }
    },
    "workforce.get": async ({ rootDir }) => {
      return {
        workforce: await workforceManager.getWorkforce(rootDir),
      }
    },
    "workforce.list": async () => {
      return {
        workforces: await workforceManager.listWorkforces(),
      }
    },
    "workforce.shutdown": async ({ rootDir }) => {
      return {
        rootDir,
        success: await workforceManager.shutdownWorkforce(rootDir),
      }
    },
    "workforce.request": async (payload) => {
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
    "workforce.update": async (payload) => {
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
    "workforce.cancel": async (payload) => {
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
    "workforce.truncate": async (payload) => {
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
    "workforce.respond": async (payload) => {
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
    "workforce.suspend": async (payload) => {
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
  } satisfies Handlers<typeof daemonIpcSchema>

  const ipcServer = createServer({
    port: runtime.port,
    schema: daemonIpcSchema,
    handlers: requestHandlers,
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
        const sessionId = typeof filter === "object" && filter && "id" in filter ? filter.id : null
        if (typeof sessionId === "string") {
          await sessionFeature.subscriberConnected(sessionId)
        }
      }
    },
    afterUnsubscribe: async ({ name, filter }) => {
      if (name === "session.message") {
        const sessionId = typeof filter === "object" && filter && "id" in filter ? filter.id : null
        if (typeof sessionId === "string") {
          await sessionFeature.subscriberDisconnected(sessionId)
        }
      }
    },
  })

  publishInboxItemEvent = (payload) => {
    ipcServer.publish("inbox.item", payload)
  }

  await once(ipcServer.server, "listening")
  const port = readBoundTcpPort(ipcServer.server)
  const daemonUrl = createDaemonUrl(port)

  sessionManager = createSessionManager({
    daemonUrl,
    agentBinDir: runtime.agentBinDir,
    configManager,
    registryService,
    inboxManager,
    idleSessionShutdownTimeoutMs: options.idleSessionShutdownTimeoutMs,
    publish(id, message) {
      ipcServer.publish("session.message", { id, message })
    },
  })

  loopManager = createLoopManager({
    sessionManager,
    resolveLoopStartRequest(input) {
      return resolveNamedLoopStartRequest(input, configManager)
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
      await loopManager.close().catch(() => {})
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

function readBoundTcpPort(server: Server) {
  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("IPC server did not bind to a TCP port")
  }

  return address.port
}
