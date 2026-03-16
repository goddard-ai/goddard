import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import { serve } from "srvx"
import { requireAuthorizedSession } from "./auth.ts"
import { HttpError } from "./errors.ts"
import { resolveReplyRequestFromGit, resolveSubmitRequestFromGit } from "./git.ts"
import {
  cleanupSocketPath,
  createDaemonUrl,
  getDefaultDaemonSocketPath,
  prepareSocketPath,
} from "./socket.ts"
import type {
  BackendPrClient,
  DaemonServer,
  DaemonServerDeps,
  ReplyPrDaemonRequest,
  SubmitPrDaemonRequest,
} from "./types.ts"
import {
  optionalNumber,
  optionalOptionalString,
  optionalString,
  requiredString,
} from "./validation.ts"

export async function startDaemonServer(
  client: BackendPrClient,
  options: { socketPath?: string } = {},
  deps: DaemonServerDeps = {},
): Promise<DaemonServer> {
  const socketPath = options.socketPath ?? getDefaultDaemonSocketPath()
  const resolveSubmitRequest = deps.resolveSubmitRequest ?? resolveSubmitRequestFromGit
  const resolveReplyRequest = deps.resolveReplyRequest ?? resolveReplyRequestFromGit
  const getSessionByToken = deps.getSessionByToken ?? SessionPermissionsStorage.getByToken
  const addAllowedPrToSession = deps.addAllowedPrToSession ?? SessionPermissionsStorage.addAllowedPr

  await prepareSocketPath(socketPath)

  const server = serve({
    manual: true,
    silent: true,
    fetch: async (request) => {
      const url = new URL(request.url)

      try {
        if (request.method === "GET" && url.pathname === "/health") {
          return Response.json({ ok: true })
        }

        if (request.method === "POST" && url.pathname === "/pr/submit") {
          const session = await requireAuthorizedSession(request, getSessionByToken)
          const body = (await request.json()) as Partial<SubmitPrDaemonRequest>
          const resolvedInput = await resolveSubmitRequest({
            cwd: requiredString(body.cwd, "cwd"),
            title: requiredString(body.title, "title"),
            body: optionalString(body.body, "body"),
            head: optionalOptionalString(body.head, "head"),
            base: optionalOptionalString(body.base, "base"),
          })

          const pr = await client.pr.create({
            ...resolvedInput,
            owner: session.owner,
            repo: session.repo,
          })
          await addAllowedPrToSession(session.sessionId, pr.number)
          return Response.json({ number: pr.number, url: pr.url })
        }

        if (request.method === "POST" && url.pathname === "/pr/reply") {
          const session = await requireAuthorizedSession(request, getSessionByToken)
          const body = (await request.json()) as Partial<ReplyPrDaemonRequest>
          const resolvedInput = await resolveReplyRequest({
            cwd: requiredString(body.cwd, "cwd"),
            message: optionalString(body.message, "message"),
            prNumber: optionalNumber(body.prNumber, "prNumber"),
          })

          if (!session.allowedPrNumbers.includes(resolvedInput.prNumber)) {
            return Response.json(
              { error: `PR #${resolvedInput.prNumber} is not allowed for this session` },
              { status: 403 },
            )
          }

          const result = await client.pr.reply({
            ...resolvedInput,
            owner: session.owner,
            repo: session.repo,
          })
          return Response.json(result)
        }

        return Response.json({ error: "Not Found" }, { status: 404 })
      } catch (error) {
        const statusCode = error instanceof HttpError ? error.statusCode : 400
        const message = error instanceof Error ? error.message : String(error)
        return Response.json({ error: message }, { status: statusCode })
      }
    },
  })

  const nodeServer = server.node?.server
  if (!nodeServer) {
    throw new Error("Daemon IPC server requires a Node.js runtime")
  }

  await new Promise<void>((resolve, reject) => {
    const onError = (error: Error) => {
      nodeServer.off("listening", onListening)
      reject(error)
    }
    const onListening = () => {
      nodeServer.off("error", onError)
      resolve()
    }

    nodeServer.once("error", onError)
    nodeServer.once("listening", onListening)
    nodeServer.listen({ path: socketPath })
  })

  let closed = false

  return {
    daemonUrl: createDaemonUrl(socketPath),
    socketPath,
    close: async () => {
      if (closed) {
        return
      }
      closed = true
      await server.close(true)
      await cleanupSocketPath(socketPath)
    },
  }
}
