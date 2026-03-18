import * as acp from "@agentclientprotocol/sdk"
import { createServer } from "@goddard-ai/ipc"
import type { CreateDaemonSessionRequest } from "@goddard-ai/schema/daemon"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import { once } from "node:events"
import { createSessionManager } from "../session/manager.ts"
import { resolveReplyRequestFromGit, resolveSubmitRequestFromGit } from "./git.ts"
import { cleanupSocketPath, createDaemonUrl, prepareSocketPath } from "./socket.ts"
import type { BackendPrClient, DaemonServer, DaemonServerDeps } from "./types.ts"
import { resolveDaemonRuntimeConfig } from "../config.ts"
import { createDaemonLogger, createPayloadPreview, readSessionIdForLog } from "../logging.ts"

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

  await prepareSocketPath(socketPath)

  let sessionManager!: ReturnType<typeof createSessionManager>

  function withRequestLogging<TPayload, TResponse>(
    requestName: string,
    handler: (
      payload: TPayload,
      context: { setSessionId: (sessionId: string) => void },
    ) => Promise<TResponse> | TResponse,
  ) {
    return async (payload: TPayload): Promise<TResponse> => {
      const opId = logger.createOpId()
      const startedAt = Date.now()
      let sessionId = readSessionIdForLog(payload)

      logger.log("ipc.request_received", {
        opId,
        requestName,
        sessionId,
        payload: createPayloadPreview(payload),
      })

      try {
        const response = await handler(payload, {
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

  const ipcServer = createServer(socketPath, daemonIpcSchema, {
    health: withRequestLogging("health", async () => ({ ok: true })),
    prSubmit: withRequestLogging("prSubmit", async (payload, context) => {
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
      return { number: pr.number, url: pr.url }
    }),
    prReply: withRequestLogging("prReply", async (payload, context) => {
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

      return client.pr.reply({
        ...resolvedInput,
        owner: session.owner,
        repo: session.repo,
      })
    }),
    sessionCreate: withRequestLogging("sessionCreate", async (payload, context) => {
      const response = {
        session: await sessionManager.createSession(payload as CreateDaemonSessionRequest),
      }
      context.setSessionId(response.session.id)
      return response
    }),
    sessionGet: withRequestLogging("sessionGet", async ({ id }) => {
      return {
        session: await sessionManager.getSession(id),
      }
    }),
    sessionConnect: withRequestLogging("sessionConnect", async ({ id }) => {
      return {
        session: await sessionManager.connectSession(id),
      }
    }),
    sessionHistory: withRequestLogging("sessionHistory", async ({ id }) => {
      return sessionManager.getHistory(id)
    }),
    sessionDiagnostics: withRequestLogging("sessionDiagnostics", async ({ id }) => {
      return sessionManager.getDiagnostics(id)
    }),
    sessionShutdown: withRequestLogging("sessionShutdown", async ({ id }) => {
      return {
        id,
        success: await sessionManager.shutdownSession(id),
      }
    }),
    sessionSend: withRequestLogging("sessionSend", async ({ id, message }) => {
      await sessionManager.sendMessage(id, message as acp.AnyMessage)
      return { accepted: true as const }
    }),
    sessionResolveToken: withRequestLogging("sessionResolveToken", async ({ token }, context) => {
      const id = await sessionManager.resolveSessionIdByToken(token)
      context.setSessionId(id)
      return {
        id,
      }
    }),
  })

  sessionManager = createSessionManager({
    daemonUrl,
    agentBinDir: runtime.agentBinDir,
    publish: (id, message) => {
      ipcServer.publish("sessionMessage", { id, message })
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
      await sessionManager.close().catch(() => {})
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error) => {
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
