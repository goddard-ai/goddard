import { randomBytes } from "node:crypto"
import type {
  AuthSession,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  ProviderIdentity,
} from "@goddard-ai/auth/schema"
import type { GitHubRepositoryRef } from "@goddard-ai/github/schema"
import type { CreatePrInput, PullRequestRecord } from "@goddard-ai/pull-request/schema"
import type { RemoteRepoStreamService } from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import { type Client } from "@libsql/client"
import { and, eq, gt } from "drizzle-orm"
import { drizzle } from "drizzle-orm/libsql"

import {
  assertRepo,
  createPrViaApp,
  HttpError,
  postPrCommentViaApp,
  type BackendControlPlane,
} from "../api/control-plane.ts"
import {
  getPrincipalDisplayName,
  sessionToPrincipal,
  type BackendPrincipal,
} from "../api/events.ts"
import type { Env } from "../env.ts"
import { hashToInteger } from "../utils.ts"
import * as schema from "./schema.ts"

/** Turso-backed backend control plane used by the real backend worker. */
export class TursoBackendControlPlane
  implements BackendControlPlane, Pick<RemoteRepoStreamService, "resolveEventOwner">
{
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
    const token = `tok_${randomBytes(32).toString("hex")}`
    const providerIdentity = input.providerIdentity
    const principal = createPrincipal(providerIdentity)
    const expiresAt = Date.now() + 1000 * 60 * 60 * 24

    await this.#db.transaction(async (tx) => {
      await tx
        .insert(schema.users)
        .values({
          id: principal.id,
          createdAt: new Date().toISOString(),
        })
        .onConflictDoNothing()

      await tx
        .insert(schema.providerIdentities)
        .values({
          provider: providerIdentity.provider,
          subject: providerIdentity.subject,
          principalId: principal.id,
          displayName: providerIdentity.displayName,
          createdAt: new Date().toISOString(),
        })
        .onConflictDoUpdate({
          target: [schema.providerIdentities.provider, schema.providerIdentities.subject],
          set: {
            principalId: principal.id,
            displayName: providerIdentity.displayName,
          },
        })

      await tx.insert(schema.authSessions).values({
        token,
        principalId: principal.id,
        expiresAt,
        createdAt: new Date().toISOString(),
      })
    })

    return { token, principal }
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
      principal: await this.#getPrincipalById(session.principalId),
    }
  }

  async getPrincipal(token: string): Promise<BackendPrincipal> {
    const session = await this.getSession(token)
    return sessionToPrincipal(
      session,
      await this.#listRepositoriesForPrincipal(session.principal.id),
    )
  }

  async getPrincipalForId(principalId: string): Promise<BackendPrincipal> {
    const [user] = await this.#db
      .select()
      .from(schema.users)
      .where(eq(schema.users.id, principalId))
      .limit(1)

    if (!user) {
      throw new HttpError(401, "Unknown principal")
    }

    const principal = await this.#getPrincipalById(user.id)
    return { ...principal, repositories: await this.#listRepositoriesForPrincipal(user.id) }
  }

  async createPr(token: string, input: CreatePrInput, env?: Env): Promise<PullRequestRecord> {
    const session = await this.getSession(token)
    assertRepo(input.owner, input.repo)
    if (!input.title.trim()) {
      throw new HttpError(400, "title is required")
    }

    const now = new Date().toISOString()
    const displayName = getPrincipalDisplayName(session.principal)
    const body = `${input.body?.trim() ?? ""}\n\nAuthored via CLI by @${displayName}`.trim()
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
        createdBy: session.principal.id,
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
      session.principal.id,
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
    principalId: string,
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
          eq(schema.pullRequests.createdBy, principalId),
        ),
      )
      .limit(1)

    return Boolean(match)
  }

  async resolveEventOwner(event: RepoEvent): Promise<string | undefined> {
    if (event.type === "pr.created") {
      return `github:${hashToInteger(event.author)}`
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

  async #listRepositoriesForPrincipal(principalId: string): Promise<GitHubRepositoryRef[]> {
    const rows = await this.#db
      .select({
        owner: schema.pullRequests.owner,
        repo: schema.pullRequests.repo,
      })
      .from(schema.pullRequests)
      .where(eq(schema.pullRequests.createdBy, principalId))

    const repositories = new Map<string, GitHubRepositoryRef>()
    for (const row of rows) {
      repositories.set(`${row.owner}/${row.repo}`, row)
    }

    return [...repositories.values()]
  }

  async #getPrincipalById(principalId: string): Promise<AuthSession["principal"]> {
    const identities = await this.#db
      .select()
      .from(schema.providerIdentities)
      .where(eq(schema.providerIdentities.principalId, principalId))

    if (identities.length === 0) {
      throw new HttpError(401, "Unknown principal")
    }

    return {
      id: principalId,
      providerIdentities: identities.map((identity) => ({
        provider: identity.provider,
        subject: identity.subject,
        displayName: identity.displayName ?? undefined,
      })),
    }
  }
}

function createPrincipal(providerIdentity: ProviderIdentity): AuthSession["principal"] {
  return {
    id: `${providerIdentity.provider}:${providerIdentity.subject}`,
    providerIdentities: [providerIdentity],
  }
}
