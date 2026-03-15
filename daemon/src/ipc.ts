import {
  SessionPermissionsStorage,
  type SessionPermissionsRecord,
} from "@goddard-ai/storage/session-permissions"
import { mkdir, rm } from "node:fs/promises"
import { request as httpRequest } from "node:http"
import { homedir } from "node:os"
import * as path from "node:path"
import { spawnSync } from "node:child_process"
import { serve } from "srvx"
import { ipcPath } from "./ipc-path.ts"

type SdkClient = {
  pr: {
    create: (input: {
      owner: string
      repo: string
      title: string
      body?: string
      head: string
      base: string
    }) => Promise<{ number: number; url: string }>
    reply: (input: {
      owner: string
      repo: string
      prNumber: number
      body: string
    }) => Promise<{ success: boolean }>
  }
}

export type SubmitPrDaemonRequest = {
  cwd: string
  title: string
  body: string
  head?: string
  base?: string
}

export type ReplyPrDaemonRequest = {
  cwd: string
  message: string
  prNumber?: number
}

export type DaemonServer = {
  daemonUrl: string
  socketPath: string
  close: () => Promise<void>
}

type AuthorizedSession = Pick<SessionPermissionsRecord, "sessionId" | "owner" | "repo" | "allowedPrNumbers">

type DaemonServerDeps = {
  resolveSubmitRequest?: (input: SubmitPrDaemonRequest) => Promise<Parameters<SdkClient["pr"]["create"]>[0]>
  resolveReplyRequest?: (
    input: ReplyPrDaemonRequest,
  ) => Promise<Parameters<SdkClient["pr"]["reply"]>[0]>
  getSessionByToken?: (token: string) => Promise<AuthorizedSession | null>
  addAllowedPrToSession?: (sessionId: string, prNumber: number) => Promise<void>
}

export function getDefaultDaemonSocketPath(home = homedir()): string {
  return ipcPath.resolve(path.posix.join(toPosixPath(home), ".goddard", "daemon.sock"))
}

export function createDaemonUrl(socketPath: string): string {
  const url = new URL("http://unix")
  url.searchParams.set("socketPath", socketPath)
  return url.toString()
}

export function readSocketPathFromDaemonUrl(rawUrl: string): string {
  let url: URL
  try {
    url = new URL(rawUrl)
  } catch {
    throw new Error("GODDARD_DAEMON_URL must be a valid URL")
  }

  if (url.protocol !== "http:" || url.hostname !== "unix") {
    throw new Error("GODDARD_DAEMON_URL must use the local daemon URL format")
  }

  const socketPath = url.searchParams.get("socketPath")
  if (!socketPath) {
    throw new Error("GODDARD_DAEMON_URL is missing socketPath")
  }

  return socketPath
}

export async function startDaemonServer(
  sdk: SdkClient,
  options: { socketPath?: string } = {},
  deps: DaemonServerDeps = {},
): Promise<DaemonServer> {
  const socketPath = options.socketPath ?? getDefaultDaemonSocketPath()
  const resolveSubmitRequest = deps.resolveSubmitRequest ?? resolveSubmitRequestFromGit
  const resolveReplyRequest = deps.resolveReplyRequest ?? resolveReplyRequestFromGit
  const getSessionByToken = deps.getSessionByToken ?? SessionPermissionsStorage.getByToken
  const addAllowedPrToSession =
    deps.addAllowedPrToSession ?? SessionPermissionsStorage.addAllowedPr

  if (process.platform !== "win32") {
    await mkdir(path.dirname(socketPath), { recursive: true })
    await ensureSocketPathAvailable(socketPath)
  }

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

          const pr = await sdk.pr.create({
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

          const result = await sdk.pr.reply({
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
      if (process.platform !== "win32") {
        await rm(socketPath, { force: true }).catch(() => {})
      }
    },
  }
}

export async function resolveSubmitRequestFromGit(
  input: SubmitPrDaemonRequest,
): Promise<Parameters<SdkClient["pr"]["create"]>[0]> {
  const repoRef = inferRepoFromGit(input.cwd)
  const { owner, repo } = splitRepo(repoRef)

  return {
    owner,
    repo,
    title: input.title,
    body: input.body,
    head: input.head || inferCurrentBranch(input.cwd),
    base: input.base || inferBaseBranch(input.cwd),
  }
}

export async function resolveReplyRequestFromGit(
  input: ReplyPrDaemonRequest,
): Promise<Parameters<SdkClient["pr"]["reply"]>[0]> {
  const repoRef = inferRepoFromGit(input.cwd)
  const { owner, repo } = splitRepo(repoRef)

  return {
    owner,
    repo,
    prNumber: input.prNumber ?? inferPrNumberFromGit(input.cwd),
    body: input.message,
  }
}

async function ensureSocketPathAvailable(socketPath: string): Promise<void> {
  try {
    await requestDaemonSocket(socketPath, "/health")
    throw new Error(`A Goddard daemon is already listening at ${socketPath}`)
  } catch (error) {
    const code = typeof error === "object" && error && "code" in error ? error.code : undefined
    if (code === "ENOENT" || code === "ECONNREFUSED") {
      await rm(socketPath, { force: true }).catch(() => {})
      return
    }

    throw error
  }
}

function requestDaemonSocket(socketPath: string, pathname: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const request = httpRequest(
      {
        socketPath,
        path: pathname,
        method: "GET",
      },
      (response) => {
        response.resume()
        resolve()
      },
    )

    request.once("error", (error) => reject(error))
    request.end()
  })
}

async function requireAuthorizedSession(
  request: Request,
  getSessionByToken: (token: string) => Promise<AuthorizedSession | null>,
): Promise<AuthorizedSession> {
  const authorization = request.headers.get("authorization")
  if (!authorization) {
    throw new HttpError(401, "Authorization header is required")
  }

  const match = authorization.match(/^Bearer\s+(.+)$/i)
  if (!match) {
    throw new HttpError(401, "Authorization header must use Bearer auth")
  }

  const session = await getSessionByToken(match[1]!)
  if (!session) {
    throw new HttpError(401, "Invalid session token")
  }

  return session
}

function requiredString(value: unknown, field: string): string {
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`${field} is required`)
  }

  return value
}

function optionalString(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw new Error(`${field} is required`)
  }

  return value
}

function optionalOptionalString(value: unknown, field: string): string | undefined {
  if (value === undefined) {
    return undefined
  }
  if (typeof value !== "string") {
    throw new Error(`${field} must be a string`)
  }
  return value
}

function optionalNumber(value: unknown, field: string): number | undefined {
  if (value === undefined) {
    return undefined
  }
  if (typeof value !== "number" || !Number.isInteger(value)) {
    throw new Error(`${field} must be an integer`)
  }
  return value
}

function inferRepoFromGit(cwd: string): string {
  const remote = runGit(cwd, ["config", "--get", "remote.origin.url"])
  const httpsMatch = remote.match(/^https:\/\/github\.com\/(.+?)\/(.+?)(\.git)?$/)
  if (httpsMatch) {
    return `${httpsMatch[1]}/${httpsMatch[2]}`
  }

  const sshMatch = remote.match(/^git@github\.com:(.+?)\/(.+?)(\.git)?$/)
  if (sshMatch) {
    return `${sshMatch[1]}/${sshMatch[2]}`
  }

  throw new Error(`Unsupported origin remote URL: ${remote}`)
}

function inferCurrentBranch(cwd: string): string {
  return runGit(cwd, ["rev-parse", "--abbrev-ref", "HEAD"])
}

function inferBaseBranch(cwd: string): string {
  try {
    const headRef = runGit(cwd, ["symbolic-ref", "refs/remotes/origin/HEAD"])
    return headRef.replace(/^refs\/remotes\/origin\//, "") || "main"
  } catch {
    return "main"
  }
}

function inferPrNumberFromGit(cwd: string): number {
  const branch = inferCurrentBranch(cwd)
  const match = branch.match(/^pr-(\d+)$/)
  if (!match) {
    throw new Error("Unable to infer PR number from current branch. Expected pr-<number>.")
  }

  return Number.parseInt(match[1], 10)
}

function runGit(cwd: string, args: string[]): string {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })

  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `git ${args.join(" ")} failed in ${cwd}`)
  }

  return result.stdout.trim()
}

function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}

function splitRepo(repoRef: string): { owner: string; repo: string } {
  const [owner, repo] = repoRef.split("/")
  if (!owner || !repo) {
    throw new Error("repo must be in owner/repo format")
  }

  return { owner, repo }
}

class HttpError extends Error {
  readonly statusCode: number

  constructor(statusCode: number, message: string) {
    super(message)
    this.statusCode = statusCode
  }
}
