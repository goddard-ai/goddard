import { randomBytes } from "node:crypto"
import type {
  AuthSession,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  ProviderIdentity,
} from "@goddard-ai/auth/schema"
import { getDefaultBackendPluginComposition } from "@goddard-ai/default-features/backend"
import type { CreatePrInput, PullRequestRecord } from "@goddard-ai/pull-request/schema"
import {
  createRemoteRepoBackendEvent,
  isRemoteRepoStreamSink,
  remoteRepoBackendEventSources,
  type RemoteRepoEventBroadcaster,
  type RemoteRepoStreamEvent,
  type RemoteRepoStreamService,
  type RemoteRepoStreamSink,
} from "@goddard-ai/remote-repo/backend"
import type { RemoteRepositoryRef, RepoEvent } from "@goddard-ai/remote-repo/schema"

import type { Env } from "../env.ts"
import { hashToInteger, toPublicSession } from "../utils.ts"
import {
  assertRepo,
  HttpError,
  postPrCommentViaProvider,
  type BackendControlPlane,
} from "./control-plane.ts"
import {
  getPrincipalDisplayName,
  getPrincipalStreamKey,
  sessionToPrincipal,
  type BackendPrincipal,
} from "./events.ts"

/** Stored auth session with an in-memory expiration timestamp. */
export type SessionRecord = AuthSession & { expiresAt: number }

/** Stored device-code session awaiting completion. */
export type DeviceSessionRecord = {
  provider: string
  loginHint: string
  createdAt: number
  expiresAt: number
}

const DEVICE_FLOW_EXPIRES_IN_SECONDS = 900
const DEVICE_FLOW_INTERVAL_SECONDS = 5
const AUTH_SESSION_TTL_MS = 1000 * 60 * 60 * 24

/** In-memory backend control plane used by local servers and tests. */
export class InMemoryBackendControlPlane
  implements BackendControlPlane, RemoteRepoStreamService, RemoteRepoEventBroadcaster
{
  #deviceSessions = new Map<string, DeviceSessionRecord>()
  #authSessions = new Map<string, SessionRecord>()
  #pullRequests: PullRequestRecord[] = []
  #streamsByUser = new Map<string, Set<RemoteRepoStreamSink>>()
  #nextPrId = 1

  startDeviceFlow(input: DeviceFlowStart = {}): DeviceFlowSession {
    const provider = input.provider?.trim() || "github"
    const loginHint = input.loginHint?.trim() || "developer"
    const deviceCode = `dev_${randomBytes(32).toString("hex")}`
    const userCode = randomBytes(4).toString("hex").toUpperCase()
    const createdAt = Date.now()

    this.#deviceSessions.set(deviceCode, {
      provider,
      loginHint,
      createdAt,
      expiresAt: createdAt + DEVICE_FLOW_EXPIRES_IN_SECONDS * 1000,
    })

    return {
      deviceCode,
      userCode,
      verificationUri: `https://auth.local/${provider}/device`,
      expiresIn: DEVICE_FLOW_EXPIRES_IN_SECONDS,
      interval: DEVICE_FLOW_INTERVAL_SECONDS,
    }
  }

  completeDeviceFlow(input: DeviceFlowComplete): AuthSession {
    const pending = this.#deviceSessions.get(input.deviceCode)
    if (!pending) {
      throw new HttpError(404, "Unknown device code")
    }

    if (pending.expiresAt <= Date.now()) {
      this.#deviceSessions.delete(input.deviceCode)
      throw new HttpError(410, "Device code expired")
    }

    const providerIdentity = input.providerIdentity
    if (providerIdentity.provider !== pending.provider) {
      throw new HttpError(400, "providerIdentity.provider must match the pending device flow")
    }

    const expiresAt = Date.now() + AUTH_SESSION_TTL_MS
    const session: SessionRecord = {
      token: `tok_${randomBytes(32).toString("hex")}`,
      principal: createPrincipal(providerIdentity),
      expiresAt,
    }

    this.#authSessions.set(session.token, session)
    this.#deviceSessions.delete(input.deviceCode)

    return toPublicSession(session)
  }

  getSession(token: string): AuthSession {
    const session = this.#authSessions.get(token)
    if (!session) {
      throw new HttpError(401, "Invalid token")
    }

    if (session.expiresAt <= Date.now()) {
      this.#authSessions.delete(token)
      throw new HttpError(401, "Session expired")
    }

    return toPublicSession(session)
  }

  getPrincipal(token: string): BackendPrincipal {
    const session = this.getSession(token)
    return sessionToPrincipal(session, this.#listRepositoriesForPrincipal(session.principal.id))
  }

  createPr(token: string, input: CreatePrInput): PullRequestRecord {
    const session = this.getSession(token)
    assertRepo(input.owner, input.repo)
    if (!input.title.trim()) {
      throw new HttpError(400, "title is required")
    }

    const prNumber = this.#pullRequests.length + 1
    const displayName = getPrincipalDisplayName(session.principal)
    const body = `${input.body?.trim() ?? ""}\n\nAuthored via CLI by @${displayName}`.trim()

    const record: PullRequestRecord = {
      id: this.#nextPrId++,
      number: prNumber,
      provider: input.provider,
      owner: input.owner,
      repo: input.repo,
      title: input.title,
      body,
      head: input.head,
      base: input.base,
      url: `https://remote-repo.local/${input.provider}/${input.owner}/${input.repo}/pull/${prNumber}`,
      createdBy: session.principal.id,
      createdAt: new Date().toISOString(),
    }

    this.#pullRequests.push(record)
    return record
  }

  async replyToPr(
    token: string,
    input: { provider: string; owner: string; repo: string; prNumber: number; body: string },
    env?: Env,
  ): Promise<void> {
    const session = this.getSession(token)
    assertRepo(input.owner, input.repo)
    if (!input.body.trim()) {
      throw new HttpError(400, "body is required")
    }

    const managed = this.isManagedPr(
      input.provider,
      input.owner,
      input.repo,
      input.prNumber,
      session.principal.id,
    )
    if (!managed) {
      throw new HttpError(403, "Cannot reply to a PR that is not managed by you")
    }

    await postPrCommentViaProvider(env, getDefaultBackendPluginComposition().providers, input)
  }

  isManagedPr(
    provider: string,
    owner: string,
    repo: string,
    prNumber: number,
    principalId: string,
  ): boolean {
    assertRepo(owner, repo)
    if (!Number.isInteger(prNumber) || prNumber <= 0) {
      throw new HttpError(400, "prNumber must be a positive integer")
    }

    return this.#pullRequests.some(
      (pullRequest) =>
        pullRequest.provider === provider &&
        pullRequest.owner === owner &&
        pullRequest.repo === repo &&
        pullRequest.number === prNumber &&
        pullRequest.createdBy === principalId,
    )
  }

  addStreamSocket(streamKey: string, socket: unknown): void {
    if (!isRemoteRepoStreamSink(socket)) {
      return
    }

    const room = this.#streamsByUser.get(streamKey) ?? new Set<RemoteRepoStreamSink>()
    room.add(socket)
    this.#streamsByUser.set(streamKey, room)
  }

  removeStreamSocket(streamKey: string, socket: unknown): void {
    if (!isRemoteRepoStreamSink(socket)) {
      return
    }

    const room = this.#streamsByUser.get(streamKey)
    room?.delete(socket)
    if (room && room.size === 0) {
      this.#streamsByUser.delete(streamKey)
    }
  }

  broadcastRemoteRepoEvent(event: RemoteRepoStreamEvent): void {
    const streamKey = this.#resolveAuthorizedStreamKey(event.payload)
    if (!streamKey) {
      return
    }

    const sockets = this.#streamsByUser.get(streamKey)
    if (!sockets) {
      return
    }

    const payload = JSON.stringify(event)
    for (const socket of sockets) {
      try {
        socket.send(payload)
      } catch {
        sockets.delete(socket)
        socket.close?.()
      }
    }

    if (sockets.size === 0) {
      this.#streamsByUser.delete(streamKey)
    }
  }

  resolveEventOwner(event: RepoEvent): string | undefined {
    if (event.type === "pr.created") {
      return toDefaultPrincipalId(event.author)
    }

    return this.#pullRequests.find(
      (pullRequest) =>
        pullRequest.owner === event.owner &&
        pullRequest.repo === event.repo &&
        pullRequest.number === event.prNumber,
    )?.createdBy
  }

  #listRepositoriesForPrincipal(principalId: string): RemoteRepositoryRef[] {
    const repositories = new Map<string, RemoteRepositoryRef>()

    for (const pullRequest of this.#pullRequests) {
      if (pullRequest.createdBy !== principalId) {
        continue
      }

      const key = `${pullRequest.owner}/${pullRequest.repo}`
      repositories.set(key, {
        provider: pullRequest.provider,
        owner: pullRequest.owner,
        repo: pullRequest.repo,
      })
    }

    return [...repositories.values()]
  }

  #resolveAuthorizedStreamKey(event: RepoEvent): string | undefined {
    const principalId = this.resolveEventOwner(event)
    if (!principalId) {
      return undefined
    }

    const session = {
      token: "",
      principal: createPrincipalFromId(principalId),
    }
    const principal = sessionToPrincipal(session, this.#listRepositoriesForPrincipal(principalId))
    if (
      !remoteRepoBackendEventSources["remote-repo"].authorize({
        principal,
        event: createRemoteRepoBackendEvent(event),
        providers: getDefaultBackendPluginComposition().providers,
      })
    ) {
      return undefined
    }

    return getPrincipalStreamKey(principal)
  }
}

function createPrincipal(providerIdentity: ProviderIdentity): AuthSession["principal"] {
  return {
    id: `${providerIdentity.provider}:${providerIdentity.subject}`,
    providerIdentities: [providerIdentity],
  }
}

function createPrincipalFromId(principalId: string): AuthSession["principal"] {
  const [provider, ...subjectParts] = principalId.split(":")
  const subject = subjectParts.join(":") || principalId
  return {
    id: principalId,
    providerIdentities: [
      {
        provider: provider || "unknown",
        subject,
      },
    ],
  }
}

function toDefaultPrincipalId(login: string): string {
  return `github:${hashToInteger(login)}`
}
