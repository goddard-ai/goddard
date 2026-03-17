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

export async function startDaemonServer(
  client: BackendPrClient,
  options: { socketPath?: string; agentBinDir?: string } = {},
  deps: DaemonServerDeps = {},
): Promise<DaemonServer> {
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

  const ipcServer = createServer(socketPath, daemonIpcSchema, {
    health: async () => ({ ok: true }),
    prSubmit: async (payload) => {
      const session = await getSessionByToken(payload.token)
      if (!session) {
        throw new Error("Invalid session token")
      }
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
    },
    prReply: async (payload) => {
      const session = await getSessionByToken(payload.token)
      if (!session) {
        throw new Error("Invalid session token")
      }
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
    },
    sessionCreate: async (payload) => {
      return {
        session: await sessionManager.createSession(payload as CreateDaemonSessionRequest),
      }
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
    sessionShutdown: async ({ id }) => {
      return {
        id,
        success: await sessionManager.shutdownSession(id),
      }
    },
    sessionSend: async ({ id, message }) => {
      await sessionManager.sendMessage(id, message as acp.AnyMessage)
      return { accepted: true as const }
    },
    sessionResolveToken: async ({ token }) => {
      return {
        id: await sessionManager.resolveSessionIdByToken(token),
      }
    },
  })

  sessionManager = createSessionManager({
    daemonUrl,
    agentBinDir: runtime.agentBinDir,
    publish: (id, message) => {
      ipcServer.publish("sessionMessage", { id, message })
    },
  })

  await once(ipcServer.server, "listening")

  let closed = false

  return {
    daemonUrl,
    socketPath,
    sessionManager,
    close: async () => {
      if (closed) {
        return
      }
      closed = true
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
    },
  }
}
