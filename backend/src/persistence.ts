import { createClient, type Client } from "@libsql/client";
import { drizzle } from "drizzle-orm/libsql";
import * as schema from "./schema.ts";
import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PullRequestRecord,
  RepoEvent
} from "@goddard-ai/schema";
import { eq, and, gt } from "drizzle-orm";
import { type BackendControlPlane, HttpError, assertRepo } from "./control-plane.ts";
import { randomUUID } from "node:crypto";

export class TursoBackendControlPlane implements BackendControlPlane {
  readonly #db: ReturnType<typeof drizzle<typeof schema>>;

  constructor(client: Client) {
    this.#db = drizzle({ client, schema });
  }

  async startDeviceFlow(input: DeviceFlowStart = {}): Promise<DeviceFlowSession> {
    const githubUsername = input.githubUsername?.trim() || "developer";
    const deviceCode = `dev_${randomUUID()}`;
    const userCode = randomUUID().slice(0, 8).toUpperCase();
    const expiresIn = 900;

    // In a real production app, we would store this in Turso or KV.
    // For now we'll focus on the core data records.
    return {
      deviceCode,
      userCode,
      verificationUri: "https://github.com/login/device",
      expiresIn,
      interval: 5
    };
  }

  async completeDeviceFlow(input: DeviceFlowComplete): Promise<AuthSession> {
    const githubUsername = input.githubUsername.trim();
    if (!githubUsername) {
      throw new HttpError(400, "githubUsername is required");
    }

    const token = `tok_${randomUUID()}`;
    const githubUserId = hashToInteger(githubUsername);
    const expiresAt = Date.now() + 1000 * 60 * 60 * 24;

    await this.#db.transaction(async (tx) => {
      await tx
        .insert(schema.users)
        .values({
          githubUserId,
          githubUsername,
          createdAt: new Date().toISOString()
        })
        .onConflictDoUpdate({
          target: schema.users.githubUserId,
          set: { githubUsername }
        });

      await tx.insert(schema.authSessions).values({
        token,
        githubUserId,
        githubUsername,
        expiresAt,
        createdAt: new Date().toISOString()
      });
    });

    return { token, githubUsername, githubUserId };
  }

  async getSession(token: string): Promise<AuthSession> {
    const [session] = await this.#db
      .select()
      .from(schema.authSessions)
      .where(
        and(
          eq(schema.authSessions.token, token),
          gt(schema.authSessions.expiresAt, Date.now())
        )
      )
      .limit(1);

    if (!session) {
      throw new HttpError(401, "Invalid or expired session");
    }

    return {
      token: session.token,
      githubUsername: session.githubUsername,
      githubUserId: session.githubUserId
    };
  }

  async createPr(token: string, input: CreatePrInput): Promise<PullRequestRecord> {
    const session = await this.getSession(token);
    assertRepo(input.owner, input.repo);
    if (!input.title.trim()) {
      throw new HttpError(400, "title is required");
    }

    const now = new Date().toISOString();
    const body = `${input.body?.trim() ?? ""}\n\nAuthored via CLI by @${session.githubUsername}`.trim();

    const [inserted] = await this.#db
      .insert(schema.pullRequests)
      .values({
        number: 0,
        owner: input.owner,
        repo: input.repo,
        title: input.title,
        body,
        head: input.head,
        base: input.base,
        url: `https://github.com/${input.owner}/${input.repo}/pull/0`,
        createdBy: session.githubUsername,
        createdAt: now
      })
      .returning();

    const finalNumber = inserted.id;
    const finalUrl = `https://github.com/${input.owner}/${input.repo}/pull/${finalNumber}`;
    await this.#db
      .update(schema.pullRequests)
      .set({ number: finalNumber, url: finalUrl })
      .where(eq(schema.pullRequests.id, inserted.id));

    return { ...inserted, number: finalNumber, url: finalUrl, body: inserted.body ?? "" };
  }

  async isManagedPr(owner: string, repo: string, prNumber: number, githubUsername: string): Promise<boolean> {
    assertRepo(owner, repo);
    if (!Number.isInteger(prNumber) || prNumber <= 0) {
      throw new HttpError(400, "prNumber must be a positive integer");
    }

    const [match] = await this.#db
      .select({ id: schema.pullRequests.id })
      .from(schema.pullRequests)
      .where(
        and(
          eq(schema.pullRequests.owner, owner),
          eq(schema.pullRequests.repo, repo),
          eq(schema.pullRequests.number, prNumber),
          eq(schema.pullRequests.createdBy, githubUsername)
        )
      )
      .limit(1);

    return Boolean(match);
  }

  async handleGitHubWebhook(event: GitHubWebhookInput): Promise<RepoEvent> {
    assertRepo(event.owner, event.repo);

    const createdAt = new Date().toISOString();

    if (event.type === "pull_request") {
      if (event.action === "closed" && event.merged) {
        return {
          type: "proposal.merged",
          owner: event.owner,
          repo: event.repo,
          prNumber: event.prNumber,
          author: event.author,
          createdAt
        };
      } else {
        throw new HttpError(400, "Unsupported pull_request action");
      }
    }

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
            createdAt
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
            createdAt
          };

    return mapped;
  }
}

function hashToInteger(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(i);
    hash |= 0;
  }
  return Math.abs(hash) + 1000;
}
