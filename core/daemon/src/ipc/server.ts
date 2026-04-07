import * as acp from "@agentclientprotocol/sdk"
import type { Handlers } from "@goddard-ai/ipc"
import { IpcClientError, createServer } from "@goddard-ai/ipc/node"
import type {
  DaemonSession,
  SubscribeDaemonWorkforceEventsRequest,
} from "@goddard-ai/schema/daemon"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { randomUUID } from "node:crypto"
import { once } from "node:events"
import { createConfigManager } from "../config-manager.ts"
import { resolveRuntimeConfig } from "../config.ts"
import { IpcRequestContext, SetupContext, type WorkforceActorContext } from "../context.ts"
import { createLogger, createPayloadPreview, readSessionIdForLog } from "../logging.ts"
import { createLoopManager, type LoopManager } from "../loop/index.ts"
import { db } from "../persistence/store.ts"
import { buildNamedActionSessionParams, resolveNamedAction } from "../resolvers/actions.ts"
import { resolveNamedLoopStartRequest } from "../resolvers/loops.ts"
import { createSessionManager, type SessionManager } from "../session/manager.ts"
import {
  discoverWorkforceInitCandidates,
  initializeWorkforce,
  resolveRepositoryRoot,
} from "../workforce/config.ts"
import { createWorkforceManager, type WorkforceManager } from "../workforce/index.ts"
import { normalizeWorkforceRootDir } from "../workforce/paths.ts"
import { resolveReplyRequestFromGit, resolveSubmitRequestFromGit } from "./git.ts"
import { cleanupSocketPath, createDaemonUrl, prepareSocketPath } from "./socket.ts"
import type { BackendPrClient, DaemonServer, DaemonServerDeps } from "./types.ts"

export async function startDaemonServer(
  client: BackendPrClient,
  options: { socketPath?: string; agentBinDir?: string } = {},
  deps: DaemonServerDeps = {},
): Promise<DaemonServer> {
  const logger = createLogger()
  const setupContext = SetupContext.get()
  const runtime =
    setupContext?.runtime ??
    resolveRuntimeConfig({
      socketPath: options.socketPath,
      agentBinDir: options.agentBinDir,
    })
  const socketPath = runtime.socketPath
  const daemonUrl = createDaemonUrl(socketPath)
  const setupConfigManager = setupContext?.configManager
  const configManager = deps.configManager ?? setupConfigManager ?? createConfigManager()
  const ownsConfigManager = deps.configManager == null && setupConfigManager == null
  const resolveSubmitRequest = deps.resolveSubmitRequest ?? resolveSubmitRequestFromGit
  const resolveReplyRequest = deps.resolveReplyRequest ?? resolveReplyRequestFromGit
  const getSessionByToken =
    deps.getSessionByToken ??
    (async (token: string) => {
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
    })
  const addAllowedPrToSession =
    deps.addAllowedPrToSession ??
    (async (sessionId: DaemonSession["id"], prNumber: number) => {
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
    })
  const recordPullRequest =
    deps.recordPullRequest ??
    (async (record) => {
      const existingRecord =
        db.pullRequests.first({
          where: {
            host: record.host,
            owner: record.owner,
            repo: record.repo,
            prNumber: record.prNumber,
          },
        }) ?? null
      return existingRecord
        ? db.pullRequests.put(existingRecord.id, record)
        : db.pullRequests.create(record)
    })

  await prepareSocketPath(socketPath)

  let sessionManager!: SessionManager
  let loopManager!: LoopManager
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

  const requestHandlers = {
    health: async () => ({ ok: true }),
    authDeviceStart: async (payload) => client.auth.startDeviceFlow(payload),
    authDeviceComplete: async (payload) => {
      const session = await client.auth.completeDeviceFlow(payload)
      db.metadata.set("authToken", session.token)
      return session
    },
    authWhoami: async () => client.auth.whoami(),
    authLogout: async () => {
      await client.auth.logout()
      db.metadata.delete("authToken")
      return { success: true as const }
    },
    prSubmit: async (payload) => {
      const session = await getSessionByToken(payload.token)
      if (!session) {
        throw new IpcClientError("Invalid session token")
      }
      const context = requireIpcRequestContext()
      context.setSessionId(session.sessionId)
      if (!session.owner || !session.repo) {
        throw new IpcClientError("Session is not scoped to a repository")
      }

      const resolvedInput = await resolveSubmitRequest({
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
      await recordPullRequest({
        host: "github",
        owner: session.owner,
        repo: session.repo,
        prNumber: pr.number,
        cwd: payload.cwd,
      })
      return { number: pr.number, url: pr.url }
    },
    prReply: async (payload) => {
      const session = await getSessionByToken(payload.token)
      if (!session) {
        throw new IpcClientError("Invalid session token")
      }
      const context = requireIpcRequestContext()
      context.setSessionId(session.sessionId)
      if (!session.owner || !session.repo) {
        throw new IpcClientError("Session is not scoped to a repository")
      }

      const resolvedInput = await resolveReplyRequest({
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
      await recordPullRequest({
        host: "github",
        owner: session.owner,
        repo: session.repo,
        prNumber: resolvedInput.prNumber,
        cwd: payload.cwd,
      })
      return response
    },
    sessionCreate: async (payload) => {
      const response = {
        session: await sessionManager.newSession({ request: payload }),
      }
      const context = requireIpcRequestContext()
      context.setSessionId(response.session.id)
      return response
    },
    sessionList: async (payload) => {
      return sessionManager.listSessions(payload)
    },
    sessionGet: async ({ id }) => {
      return {
        session: await sessionManager.getSession(id),
      }
    },
    sessionConnect: async ({ id }) => {
      return {
        session: await sessionManager.connectSession(id),
      }
    },
    sessionHistory: async ({ id }) => {
      return sessionManager.getHistory(id)
    },
    sessionDiagnostics: async ({ id }) => {
      return sessionManager.getDiagnostics(id)
    },
    sessionWorktree: async ({ id }) => {
      return sessionManager.getWorktree(id)
    },
    sessionWorkforce: async ({ id }) => {
      return sessionManager.getWorkforce(id)
    },
    sessionShutdown: async ({ id }) => {
      return {
        id,
        success: await sessionManager.shutdownSession(id),
      }
    },
    sessionCancel: async ({ id }) => {
      return sessionManager.cancelSessionTurn(id)
    },
    sessionSteer: async ({ id, prompt }) => {
      return sessionManager.steerSession(id, prompt)
    },
    sessionSend: async ({ id, message }) => {
      await sessionManager.sendMessage(id, message as acp.AnyMessage)
      return { accepted: true as const }
    },
    sessionResolveToken: async ({ token }) => {
      const id = await sessionManager.resolveSessionIdByToken(token)
      const context = requireIpcRequestContext()
      context.setSessionId(id)
      return {
        id,
      }
    },
    actionRun: async (payload) => {
      const action = await resolveNamedAction(payload.actionName, payload.cwd, configManager)
      const session = await sessionManager.newSession({
        request: buildNamedActionSessionParams(action, payload.cwd, {
          cwd: payload.cwd,
          agent: payload.agent,
          mcpServers: payload.mcpServers,
          env: payload.env,
          systemPrompt: payload.systemPrompt,
          repository: payload.repository,
          prNumber: payload.prNumber,
          metadata: payload.metadata,
        }),
      })
      const context = requireIpcRequestContext()
      context.setSessionId(session.id)
      return { session }
    },
    loopStart: async (payload) => {
      return {
        loop: await loopManager.startLoop(payload),
      }
    },
    loopGet: async ({ rootDir, loopName }) => {
      return {
        loop: await loopManager.getLoop(rootDir, loopName),
      }
    },
    loopList: async () => {
      return {
        loops: await loopManager.listLoops(),
      }
    },
    loopShutdown: async ({ rootDir, loopName }) => {
      return {
        rootDir,
        loopName,
        success: await loopManager.shutdownLoop(rootDir, loopName),
      }
    },
    workforceStart: async ({ rootDir }) => {
      return {
        workforce: await workforceManager.startWorkforce(rootDir),
      }
    },
    workforceDiscoverCandidates: async ({ rootDir }) => {
      // Canonicalize the repo root inside the daemon so SDK and CLI callers cannot drift.
      const repositoryRoot = await resolveRepositoryRoot(rootDir)
      return {
        rootDir: repositoryRoot,
        candidates: await discoverWorkforceInitCandidates(repositoryRoot),
      }
    },
    workforceInitialize: async ({ rootDir, packageDirs }) => {
      // Re-resolve here for the same reason as discovery: the daemon owns the canonical root.
      const repositoryRoot = await resolveRepositoryRoot(rootDir)
      return {
        initialized: await initializeWorkforce(repositoryRoot, packageDirs),
      }
    },
    workforceGet: async ({ rootDir }) => {
      return {
        workforce: await workforceManager.getWorkforce(rootDir),
      }
    },
    workforceList: async () => {
      return {
        workforces: await workforceManager.listWorkforces(),
      }
    },
    workforceShutdown: async ({ rootDir }) => {
      return {
        rootDir,
        success: await workforceManager.shutdownWorkforce(rootDir),
      }
    },
    workforceRequest: async (payload) => {
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
    workforceUpdate: async (payload) => {
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
    workforceCancel: async (payload) => {
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
    workforceTruncate: async (payload) => {
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
    workforceRespond: async (payload) => {
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
    workforceSuspend: async (payload) => {
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
    socketPath,
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
        errorMessage: error instanceof Error ? error.message : String(error),
      })
    },
    beforeSubscribe: async ({ name, filter }) => {
      if (name !== "workforceEvent") {
        return
      }

      const request = filter as SubscribeDaemonWorkforceEventsRequest | undefined
      if (!request) {
        throw new IpcClientError("Missing workforce event filter")
      }

      // Subscription setup only validates that this workforce is active; events are still pushed later
      // from the runtime when new ledger activity is appended.
      await workforceManager.getWorkforce(request.rootDir)
    },
  })

  sessionManager = createSessionManager({
    daemonUrl,
    agentBinDir: runtime.agentBinDir,
    configManager,
    publish: (id, message) => {
      ipcServer.publish("sessionMessage", { id, message })
    },
  })
  loopManager = (deps.createLoopManager ?? createLoopManager)({
    sessionManager,
    resolveLoopStartRequest: (input) => resolveNamedLoopStartRequest(input, configManager),
  })
  workforceManager = (deps.createWorkforceManager ?? ((input) => createWorkforceManager(input)))({
    sessionManager,
    publishEvent(payload) {
      ipcServer.publish("workforceEvent", payload)
    },
  })

  await once(ipcServer.server, "listening")
  logger.log("ipc.server_listening", {
    socketPath,
    daemonUrl,
  })

  let closed = false

  return {
    daemonUrl,
    socketPath,
    close: async () => {
      if (closed) {
        return
      }
      closed = true
      logger.log("ipc.server_closing", {
        socketPath,
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
      await cleanupSocketPath(socketPath)
      logger.log("ipc.server_closed", {
        socketPath,
        daemonUrl,
      })
    },
  }
}
