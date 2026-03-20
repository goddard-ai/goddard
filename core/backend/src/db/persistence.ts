import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PullRequestRecord,
  RepoEvent,
} from "@goddard-ai/schema/backend"
import { type Client } from "@libsql/client"
import { and, eq, gt } from "drizzle-orm"
import { drizzle } from "drizzle-orm/libsql"
import { randomBytes } from "node:crypto"
import {
  assertRepo,
  type BackendControlPlane,
  createPrViaApp,
  HttpError,
  postPrCommentViaApp,
} from "../api/control-plane.js"
import type { Env } from "../env.js"
import { hashToInteger } from "../utils.js"
import * as schema from "./schema.js"

/** Turso-backed backend control plane used by the real backend worker. */
export class TursoBackendControlPlane implements BackendControlPlane {
  readonly #db: ReturnType<typeof drizzle<typeof schema>>

  constructor(client: Client, _env?: Env) {
    this.#db = drizzle({ client, schema })
  }

  async startDeviceFlow(_input: DeviceFlowStart = {}): Promise<DeviceFlowSession> {
    const deviceCode = `dev_${randomBytes(32).toString("hex")}`
    const userCode = randomBytes(4).toString("hex").toUpperCase()
    const expiresIn = 900

    // In a real production app, we would store this in Turso or KV.
    // For now we'll focus on the core data records.
    return {
      deviceCode,
      userCode,
      verificationUri: "https://github.com/login/device",
      expiresIn,
      interval: 5,
    }
  }

  async completeDeviceFlow(input: DeviceFlowComplete): Promise<AuthSession> {
    const githubUsername = input.githubUsername.trim()
    if (!githubUsername) {
      throw new HttpError(400, "githubUsername is required")
    }

    const token = `tok_${randomBytes(32).toString("hex")}`
    const githubUserId = hashToInteger(githubUsername)
    const expiresAt = Date.now() + 1000 * 60 * 60 * 24

    await this.#db.transaction(async (tx) => {
      await tx
        .insert(schema.users)
        .values({
          githubUserId,
          githubUsername,
          createdAt: new Date().toISOString(),
        })
        .onConflictDoUpdate({
          target: schema.users.githubUserId,
          set: { githubUsername },
        })

      await tx.insert(schema.authSessions).values({
        token,
        githubUserId,
        githubUsername,
        expiresAt,
        createdAt: new Date().toISOString(),
      })
    })

    return { token, githubUsername, githubUserId }
  }

  async getSession(token: string): Promise<AuthSession> {
    const [session] = await this.#db
      .select()
      .from(schema.authSessions)
      .where(
        and(eq(schema.authSessions.token, token), gt(schema.authSessions.expiresAt, Date.now())),
      )
      .limit(1)

    if (!session) {
      throw new HttpError(401, "Invalid or expired session")
    }

    return {
      token: session.token,
      githubUsername: session.githubUsername,
      githubUserId: session.githubUserId,
    }
  }

  async createPr(token: string, input: CreatePrInput, env?: Env): Promise<PullRequestRecord> {
    const session = await this.getSession(token)
    assertRepo(input.owner, input.repo)
    if (!input.title.trim()) {
      throw new HttpError(400, "title is required")
    }

    const now = new Date().toISOString()
    const body =
      `${input.body?.trim() ?? ""}\n\nAuthored via CLI by @${session.githubUsername}`.trim()
    const createdPr = await createPrViaApp(env, input, body)

    const [inserted] = await this.#db
      .insert(schema.pullRequests)
      .values({
        number: createdPr.number,
        owner: input.owner,
        repo: input.repo,
        title: input.title,
        body,
        head: input.head,
        base: input.base,
        url: createdPr.url,
        createdBy: session.githubUsername,
        createdAt: createdPr.createdAt || now,
      })
      .returning()

    return { ...inserted, body: inserted.body ?? "" }
  }

  async replyToPr(
    token: string,
    input: { owner: string; repo: string; prNumber: number; body: string },
    env?: Env,
  ): Promise<void> {
    const session = await this.getSession(token)
    assertRepo(input.owner, input.repo)
    if (!input.body.trim()) {
      throw new HttpError(400, "body is required")
    }

    const managed = await this.isManagedPr(
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

  async isManagedPr(
    owner: string,
    repo: string,
    prNumber: number,
    githubUsername: string,
  ): Promise<boolean> {
    assertRepo(owner, repo)
    if (!Number.isInteger(prNumber) || prNumber <= 0) {
      throw new HttpError(400, "prNumber must be a positive integer")
    }

    const [match] = await this.#db
      .select({ id: schema.pullRequests.id })
      .from(schema.pullRequests)
      .where(
        and(
          eq(schema.pullRequests.owner, owner),
          eq(schema.pullRequests.repo, repo),
          eq(schema.pullRequests.number, prNumber),
          eq(schema.pullRequests.createdBy, githubUsername),
        ),
      )
      .limit(1)

    return Boolean(match)
  }

  async handleGitHubWebhook(event: GitHubWebhookInput): Promise<RepoEvent> {
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

  async resolveEventOwner(event: RepoEvent): Promise<string | undefined> {
    if (event.type === "pr.created") {
      return event.author
    }

    const [match] = await this.#db
      .select({ createdBy: schema.pullRequests.createdBy })
      .from(schema.pullRequests)
      .where(
        and(
          eq(schema.pullRequests.owner, event.owner),
          eq(schema.pullRequests.repo, event.repo),
          eq(schema.pullRequests.number, event.prNumber),
        ),
      )
      .limit(1)

    return match?.createdBy
  }
}
