import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PullRequestRecord,
  RepoEvent,
  RepoEventRecord,
} from "@goddard-ai/schema/backend"
import { randomBytes } from "node:crypto"
import type { Env } from "../env.js"
import { hashToInteger, toPublicSession } from "../utils.js"
import {
  type BackendControlPlane,
  HttpError,
  type PersistedRepoEvent,
  assertRepo,
  postPrCommentViaApp,
} from "./control-plane.js"

/** Stored auth session with an in-memory expiration timestamp. */
export type SessionRecord = AuthSession & { expiresAt: number }

/** Stored device-code session awaiting completion. */
export type DeviceSessionRecord = { githubUsername: string; createdAt: number; expiresAt: number }

/** Minimal sink interface used by local live-stream fanout. */
export type StreamSink = {
  send: (message: { data: string; id?: string | number }) => void
  close?: () => void
}

const DEVICE_FLOW_EXPIRES_IN_SECONDS = 900
const DEVICE_FLOW_INTERVAL_SECONDS = 5
const AUTH_SESSION_TTL_MS = 1000 * 60 * 60 * 24

/** In-memory backend control plane used by local servers and tests. */
export class InMemoryBackendControlPlane implements BackendControlPlane {
  #deviceSessions = new Map<string, DeviceSessionRecord>()
  #authSessions = new Map<string, SessionRecord>()
  #pullRequests: PullRequestRecord[] = []
  #repoEvents: PersistedRepoEvent[] = []
  #streamsByUser = new Map<string, Set<StreamSink>>()
  #nextPrId = 1
  #nextRepoEventId = 1

  startDeviceFlow(input: DeviceFlowStart = {}): DeviceFlowSession {
    const githubUsername = input.githubUsername?.trim() || "developer"
    const deviceCode = `dev_${randomBytes(32).toString("hex")}`
    const userCode = randomBytes(4).toString("hex").toUpperCase()
    const createdAt = Date.now()

    this.#deviceSessions.set(deviceCode, {
      githubUsername,
      createdAt,
      expiresAt: createdAt + DEVICE_FLOW_EXPIRES_IN_SECONDS * 1000,
    })

    return {
      deviceCode,
      userCode,
      verificationUri: "https://github.com/login/device",
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

    const githubUsername = input.githubUsername.trim()
    if (!githubUsername) {
      throw new HttpError(400, "githubUsername is required")
    }

    const expiresAt = Date.now() + AUTH_SESSION_TTL_MS
    const session: SessionRecord = {
      token: `tok_${randomBytes(32).toString("hex")}`,
      githubUsername,
      githubUserId: hashToInteger(githubUsername),
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

  createPr(token: string, input: CreatePrInput): PullRequestRecord {
    const session = this.getSession(token)
    assertRepo(input.owner, input.repo)
    if (!input.title.trim()) {
      throw new HttpError(400, "title is required")
    }

    const prNumber = this.#pullRequests.length + 1
    const body =
      `${input.body?.trim() ?? ""}\n\nAuthored via CLI by @${session.githubUsername}`.trim()

    const record: PullRequestRecord = {
      id: this.#nextPrId++,
      number: prNumber,
      owner: input.owner,
      repo: input.repo,
      title: input.title,
      body,
      head: input.head,
      base: input.base,
      url: `https://github.com/${input.owner}/${input.repo}/pull/${prNumber}`,
      createdBy: session.githubUsername,
      createdAt: new Date().toISOString(),
    }

    this.#pullRequests.push(record)
    return record
  }

  async replyToPr(
    token: string,
    input: { owner: string; repo: string; prNumber: number; body: string },
    env?: Env,
  ): Promise<void> {
    const session = this.getSession(token)
    assertRepo(input.owner, input.repo)
    if (!input.body.trim()) {
      throw new HttpError(400, "body is required")
    }

    const managed = this.isManagedPr(
      input.owner,
      input.repo,
      input.prNumber,
      session.githubUsername,
    )
    if (!managed) {
      throw new HttpError(403, "Cannot reply to a PR that is not managed by you")
    }

    await postPrCommentViaApp(env, input.owner, input.repo, input.prNumber, input.body)
  }

  isManagedPr(owner: string, repo: string, prNumber: number, githubUsername: string): boolean {
    assertRepo(owner, repo)
    if (!Number.isInteger(prNumber) || prNumber <= 0) {
      throw new HttpError(400, "prNumber must be a positive integer")
    }

    return this.#pullRequests.some(
      (pullRequest) =>
        pullRequest.owner === owner &&
        pullRequest.repo === repo &&
        pullRequest.number === prNumber &&
        pullRequest.createdBy === githubUsername,
    )
  }

  handleGitHubWebhook(event: GitHubWebhookInput): RepoEvent {
    assertRepo(event.owner, event.repo)

    const createdAt = new Date().toISOString()
    const mapped: RepoEvent =
      event.type === "issue_comment"
        ? {
            type: "comment",
            owner: event.owner,
            repo: event.repo,
            prNumber: event.prNumber,
            author: event.author,
            body: event.body,
            reactionAdded: "eyes",
            createdAt,
          }
        : {
            type: "review",
            owner: event.owner,
            repo: event.repo,
            prNumber: event.prNumber,
            author: event.author,
            state: event.state,
            body: event.body,
            reactionAdded: "eyes",
            createdAt,
          }

    return mapped
  }

  getRepoEventHistory(token: string, after?: number): RepoEventRecord[] {
    const session = this.getSession(token)
    return this.#repoEvents
      .filter(
        ({ githubUsername, record }) =>
          githubUsername === session.githubUsername && (after === undefined || record.id > after),
      )
      .map(({ record }) => record)
  }

  recordRepoEvent(event: RepoEvent): PersistedRepoEvent | null {
    const githubUsername = this.resolveEventOwner(event)
    if (!githubUsername) {
      return null
    }

    const persistedEvent: PersistedRepoEvent = {
      githubUsername,
      record: {
        id: this.#nextRepoEventId++,
        createdAt: new Date().toISOString(),
        event,
      },
    }

    this.#repoEvents.push(persistedEvent)
    return persistedEvent
  }

  addStreamSocket(githubUsername: string, socket: unknown): void {
    if (!isStreamSink(socket)) {
      return
    }

    const room = this.#streamsByUser.get(githubUsername) ?? new Set<StreamSink>()
    room.add(socket)
    this.#streamsByUser.set(githubUsername, room)
  }

  removeStreamSocket(githubUsername: string, socket: unknown): void {
    if (!isStreamSink(socket)) {
      return
    }

    const room = this.#streamsByUser.get(githubUsername)
    room?.delete(socket)
    if (room && room.size === 0) {
      this.#streamsByUser.delete(githubUsername)
    }
  }

  broadcast(persistedEvent: PersistedRepoEvent): void {
    const { githubUsername, record } = persistedEvent
    const sockets = this.#streamsByUser.get(githubUsername)
    if (!sockets) {
      return
    }

    const payload = JSON.stringify({ event: record.event })
    for (const socket of sockets) {
      try {
        socket.send({ data: payload, id: record.id })
      } catch {
        sockets.delete(socket)
        socket.close?.()
      }
    }

    if (sockets.size === 0) {
      this.#streamsByUser.delete(githubUsername)
    }
  }

  resolveEventOwner(event: RepoEvent): string | undefined {
    if (event.type === "pr.created") {
      return event.author
    }

    return this.#pullRequests.find(
      (pullRequest) =>
        pullRequest.owner === event.owner &&
        pullRequest.repo === event.repo &&
        pullRequest.number === event.prNumber,
    )?.createdBy
  }
}

/** Returns whether a value supports the minimal send/close contract used by live-stream fanout. */
export function isStreamSink(value: unknown): value is StreamSink {
  return (
    !!value &&
    typeof value === "object" &&
    "send" in value &&
    typeof (value as StreamSink).send === "function"
  )
}
