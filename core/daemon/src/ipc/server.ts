import * as acp from "@agentclientprotocol/sdk"
import { createServer } from "@goddard-ai/ipc"
import type {
  CreateDaemonSessionRequest,
  ListDaemonSessionsRequest,
  StartDaemonLoopRequest,
} from "@goddard-ai/schema/daemon"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { SessionStorage } from "@goddard-ai/storage"
import { ManagedPrLocationStorage } from "@goddard-ai/storage/managed-pr-locations"
import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import { once } from "node:events"
import { resolveDaemonRuntimeConfig } from "../config.ts"
import { createDaemonLogger, createPayloadPreview, readSessionIdForLog } from "../logging.ts"
import { createLoopManager } from "../loop/index.ts"
import { createSessionManager } from "../session/manager.ts"
import { createWorkforceManager, type WorkforceActorContext } from "../workforce/index.ts"
import { resolveReplyRequestFromGit, resolveSubmitRequestFromGit } from "./git.ts"
import { cleanupSocketPath, createDaemonUrl, prepareSocketPath } from "./socket.ts"
import type { BackendPrClient, DaemonServer, DaemonServerDeps } from "./types.ts"

export async function startDaemonServer(
  client: BackendPrClient,
  options: { socketPath?: string; agentBinDir?: string } = {},
  deps: DaemonServerDeps = {},
): Promise<DaemonServer> {
  const logger = createDaemonLogger()
  const runtime = resolveDaemonRuntimeConfig({
    socketPath: options.socketPath,
    agentBinDir: options.agentBinDir,
  })
  const socketPath = runtime.socketPath
  const daemonUrl = createDaemonUrl(socketPath)
  const resolveSubmitRequest = deps.resolveSubmitRequest ?? resolveSubmitRequestFromGit
  const resolveReplyRequest = deps.resolveReplyRequest ?? resolveReplyRequestFromGit
  const getSessionByToken = deps.getSessionByToken ?? SessionPermissionsStorage.getByToken
  const addAllowedPrToSession = deps.addAllowedPrToSession ?? SessionPermissionsStorage.addAllowedPr
  const recordManagedPrLocation = deps.recordManagedPrLocation ?? ManagedPrLocationStorage.upsert

  await prepareSocketPath(socketPath)

  let sessionManager!: ReturnType<typeof createSessionManager>
  let loopManager!: ReturnType<typeof createLoopManager>
  let workforceManager!: ReturnType<typeof createWorkforceManager>

  async function resolveWorkforceActor(
    token: string | undefined,
    context: { setSessionId: (sessionId: string) => void },
  ): Promise<WorkforceActorContext> {
    if (!token) {
      return {
        sessionId: null,
        agentId: null,
        requestId: null,
      }
    }

    const session = await getSessionByToken(token)
    if (!session) {
      throw new Error("Invalid session token")
    }

    context.setSessionId(session.sessionId)

    const sessionRecord = await SessionStorage.get(session.sessionId)
    const metadata =
      sessionRecord && typeof sessionRecord.metadata === "object" && sessionRecord.metadata !== null
        ? (sessionRecord.metadata as {
            workforce?: {
              agentId?: string
              requestId?: string
            }
          })
        : null

    if (!metadata?.workforce || typeof metadata.workforce.agentId !== "string") {
      throw new Error("Session is not attached to a workforce request")
    }

    return {
      sessionId: session.sessionId,
      agentId: metadata.workforce.agentId,
      requestId:
        typeof metadata.workforce.requestId === "string" ? metadata.workforce.requestId : null,
    }
  }

  function requireActorRequestId(actor: WorkforceActorContext): string {
    if (!actor.requestId) {
      throw new Error("Session is not attached to an active workforce request")
    }

    return actor.requestId
  }

  function withRequestLogging<TPayload, TResponse>(
    requestName: string,
    handler: (
      payload: TPayload,
      context: { setSessionId: (sessionId: string) => void },
    ) => Promise<TResponse> | TResponse,
  ) {
    return async (payload: unknown): Promise<TResponse> => {
      const typedPayload = payload as TPayload
      const opId = logger.createOpId()
      const startedAt = Date.now()
      let sessionId = readSessionIdForLog(typedPayload)

      logger.log("ipc.request_received", {
        opId,
        requestName,
        sessionId,
        payload: createPayloadPreview(typedPayload),
      })

      try {
        const response = await handler(typedPayload, {
          setSessionId(nextSessionId) {
            sessionId = nextSessionId
          },
        })

        logger.log("ipc.response_sent", {
          opId,
          requestName,
          sessionId: sessionId ?? readSessionIdForLog(response),
          durationMs: Date.now() - startedAt,
          response: createPayloadPreview(response),
        })

        return response
      } catch (error) {
        logger.log("ipc.request_failed", {
          opId,
          requestName,
          sessionId,
          durationMs: Date.now() - startedAt,
          errorMessage: error instanceof Error ? error.message : String(error),
        })
        throw error
      }
    }
  }

  // Keep app/src/daemon-ipc-test-handlers.ts in sync with this handler list.
  // When new daemon IPC methods are added here, update the app test stub as well.
  const ipcServer = createServer(socketPath, daemonIpcSchema, {
    health: withRequestLogging<{}, { ok: true }>("health", async () => ({ ok: true })),
    prSubmit: withRequestLogging<
      {
        token: string
        cwd: string
        title: string
        body: string
        head?: string
        base?: string
      },
      { number: number; url: string }
    >("prSubmit", async (payload, context) => {
      const session = await getSessionByToken(payload.token)
      if (!session) {
        throw new Error("Invalid session token")
      }
      context.setSessionId(session.sessionId)
      if (!session.owner || !session.repo) {
        throw new Error("Session is not scoped to a repository")
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
      await recordManagedPrLocation({
        owner: session.owner,
        repo: session.repo,
        prNumber: pr.number,
        cwd: payload.cwd,
      })
      return { number: pr.number, url: pr.url }
    }),
    prReply: withRequestLogging<
      {
        token: string
        cwd: string
        message: string
        prNumber?: number
      },
      { success: boolean }
    >("prReply", async (payload, context) => {
      const session = await getSessionByToken(payload.token)
      if (!session) {
        throw new Error("Invalid session token")
      }
      context.setSessionId(session.sessionId)
      if (!session.owner || !session.repo) {
        throw new Error("Session is not scoped to a repository")
      }

      const resolvedInput = await resolveReplyRequest({
        cwd: payload.cwd,
        message: payload.message,
        prNumber: payload.prNumber,
      })

      if (!session.allowedPrNumbers.includes(resolvedInput.prNumber)) {
        throw new Error(`PR #${resolvedInput.prNumber} is not allowed for this session`)
      }

      const response = await client.pr.reply({
        ...resolvedInput,
        owner: session.owner,
        repo: session.repo,
      })
      await recordManagedPrLocation({
        owner: session.owner,
        repo: session.repo,
        prNumber: resolvedInput.prNumber,
        cwd: payload.cwd,
      })
      return response
    }),
    sessionCreate: withRequestLogging<
      CreateDaemonSessionRequest,
      { session: Awaited<ReturnType<typeof sessionManager.newSession>> }
    >("sessionCreate", async (payload, context) => {
      const response = {
        session: await sessionManager.newSession(payload),
      }
      context.setSessionId(response.session.id)
      return response
    }),
    sessionList: withRequestLogging<
      ListDaemonSessionsRequest,
      Awaited<ReturnType<typeof sessionManager.listSessions>>
    >("sessionList", async (payload) => {
      return sessionManager.listSessions(payload)
    }),
    sessionGet: withRequestLogging<
      { id: string },
      { session: Awaited<ReturnType<typeof sessionManager.getSession>> }
    >("sessionGet", async ({ id }) => {
      return {
        session: await sessionManager.getSession(id),
      }
    }),
    sessionConnect: withRequestLogging<
      { id: string },
      { session: Awaited<ReturnType<typeof sessionManager.connectSession>> }
    >("sessionConnect", async ({ id }) => {
      return {
        session: await sessionManager.connectSession(id),
      }
    }),
    sessionHistory: withRequestLogging<
      { id: string },
      Awaited<ReturnType<typeof sessionManager.getHistory>>
    >("sessionHistory", async ({ id }) => {
      return sessionManager.getHistory(id)
    }),
    sessionDiagnostics: withRequestLogging<
      { id: string },
      Awaited<ReturnType<typeof sessionManager.getDiagnostics>>
    >("sessionDiagnostics", async ({ id }) => {
      return sessionManager.getDiagnostics(id)
    }),
    sessionShutdown: withRequestLogging<{ id: string }, { id: string; success: boolean }>(
      "sessionShutdown",
      async ({ id }) => {
        return {
          id,
          success: await sessionManager.shutdownSession(id),
        }
      },
    ),
    sessionSend: withRequestLogging<{ id: string; message: unknown }, { accepted: true }>(
      "sessionSend",
      async ({ id, message }) => {
        await sessionManager.sendMessage(id, message as acp.AnyMessage)
        return { accepted: true as const }
      },
    ),
    sessionResolveToken: withRequestLogging<{ token: string }, { id: string }>(
      "sessionResolveToken",
      async ({ token }, context) => {
        const id = await sessionManager.resolveSessionIdByToken(token)
        context.setSessionId(id)
        return {
          id,
        }
      },
    ),
    loopStart: withRequestLogging<
      StartDaemonLoopRequest,
      { loop: Awaited<ReturnType<typeof loopManager.startLoop>> }
    >("loopStart", async (payload) => {
      return {
        loop: await loopManager.startLoop(payload),
      }
    }),
    loopGet: withRequestLogging<
      { rootDir: string; loopName: string },
      { loop: Awaited<ReturnType<typeof loopManager.getLoop>> }
    >("loopGet", async ({ rootDir, loopName }) => {
      return {
        loop: await loopManager.getLoop(rootDir, loopName),
      }
    }),
    loopList: withRequestLogging<{}, { loops: Awaited<ReturnType<typeof loopManager.listLoops>> }>(
      "loopList",
      async () => {
        return {
          loops: await loopManager.listLoops(),
        }
      },
    ),
    loopShutdown: withRequestLogging<
      { rootDir: string; loopName: string },
      { rootDir: string; loopName: string; success: boolean }
    >("loopShutdown", async ({ rootDir, loopName }) => {
      return {
        rootDir,
        loopName,
        success: await loopManager.shutdownLoop(rootDir, loopName),
      }
    }),
    workforceStart: withRequestLogging<
      { rootDir: string },
      { workforce: Awaited<ReturnType<typeof workforceManager.startWorkforce>> }
    >("workforceStart", async ({ rootDir }) => {
      return {
        workforce: await workforceManager.startWorkforce(rootDir),
      }
    }),
    workforceGet: withRequestLogging<
      { rootDir: string },
      { workforce: Awaited<ReturnType<typeof workforceManager.getWorkforce>> }
    >("workforceGet", async ({ rootDir }) => {
      return {
        workforce: await workforceManager.getWorkforce(rootDir),
      }
    }),
    workforceList: withRequestLogging<
      {},
      { workforces: Awaited<ReturnType<typeof workforceManager.listWorkforces>> }
    >("workforceList", async () => {
      return {
        workforces: await workforceManager.listWorkforces(),
      }
    }),
    workforceShutdown: withRequestLogging<
      { rootDir: string },
      { rootDir: string; success: boolean }
    >("workforceShutdown", async ({ rootDir }) => {
      return {
        rootDir,
        success: await workforceManager.shutdownWorkforce(rootDir),
      }
    }),
    workforceRequest: withRequestLogging<
      {
        rootDir: string
        targetAgentId: string
        input: string
        intent?: "default" | "create"
        token?: string
      },
      Awaited<ReturnType<typeof workforceManager.appendWorkforceEvent>>
    >("workforceRequest", async (payload, context) => {
      const actor = await resolveWorkforceActor(payload.token, context)
      return workforceManager.appendWorkforceEvent(
        payload.rootDir,
        {
          type: "request",
          targetAgentId: payload.targetAgentId,
          input: payload.input,
          intent: payload.intent,
        },
        actor,
      )
    }),
    workforceUpdate: withRequestLogging<
      { rootDir: string; requestId: string; input: string; token?: string },
      Awaited<ReturnType<typeof workforceManager.appendWorkforceEvent>>
    >("workforceUpdate", async (payload, context) => {
      const actor = await resolveWorkforceActor(payload.token, context)
      return workforceManager.appendWorkforceEvent(
        payload.rootDir,
        {
          type: "update",
          requestId: payload.requestId,
          input: payload.input,
        },
        actor,
      )
    }),
    workforceCancel: withRequestLogging<
      { rootDir: string; requestId: string; reason?: string; token?: string },
      Awaited<ReturnType<typeof workforceManager.appendWorkforceEvent>>
    >("workforceCancel", async (payload, context) => {
      const actor = await resolveWorkforceActor(payload.token, context)
      return workforceManager.appendWorkforceEvent(
        payload.rootDir,
        {
          type: "cancel",
          requestId: payload.requestId,
          reason: payload.reason ?? null,
        },
        actor,
      )
    }),
    workforceTruncate: withRequestLogging<
      { rootDir: string; agentId?: string; reason?: string; token?: string },
      Awaited<ReturnType<typeof workforceManager.appendWorkforceEvent>>
    >("workforceTruncate", async (payload, context) => {
      const actor = await resolveWorkforceActor(payload.token, context)
      return workforceManager.appendWorkforceEvent(
        payload.rootDir,
        {
          type: "truncate",
          agentId: payload.agentId ?? null,
          reason: payload.reason ?? null,
        },
        actor,
      )
    }),
    workforceRespond: withRequestLogging<
      { rootDir: string; output: string; token: string },
      Awaited<ReturnType<typeof workforceManager.appendWorkforceEvent>>
    >("workforceRespond", async (payload, context) => {
      const actor = await resolveWorkforceActor(payload.token, context)
      return workforceManager.appendWorkforceEvent(
        payload.rootDir,
        {
          type: "respond",
          requestId: requireActorRequestId(actor),
          output: payload.output,
        },
        actor,
      )
    }),
    workforceSuspend: withRequestLogging<
      { rootDir: string; reason: string; token: string },
      Awaited<ReturnType<typeof workforceManager.appendWorkforceEvent>>
    >("workforceSuspend", async (payload, context) => {
      const actor = await resolveWorkforceActor(payload.token, context)
      return workforceManager.appendWorkforceEvent(
        payload.rootDir,
        {
          type: "suspend",
          requestId: requireActorRequestId(actor),
          reason: payload.reason,
        },
        actor,
      )
    }),
  })

  sessionManager = createSessionManager({
    daemonUrl,
    agentBinDir: runtime.agentBinDir,
    publish: (id, message) => {
      ipcServer.publish("sessionMessage", { id, message })
    },
  })
  loopManager = (deps.createLoopManager ?? ((input) => createLoopManager(input)))({
    sessionManager,
  })
  workforceManager = (deps.createWorkforceManager ?? ((input) => createWorkforceManager(input)))({
    sessionManager,
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
