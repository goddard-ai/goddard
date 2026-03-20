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
import type { Env } from "../env.js"

/** Backend operations that the HTTP router can delegate to a storage implementation. */
export interface BackendControlPlane {
  startDeviceFlow(input?: DeviceFlowStart): Promise<DeviceFlowSession> | DeviceFlowSession
  completeDeviceFlow(input: DeviceFlowComplete): Promise<AuthSession> | AuthSession
  getSession(token: string): Promise<AuthSession> | AuthSession
  createPr(
    token: string,
    input: CreatePrInput,
    env?: Env,
  ): Promise<PullRequestRecord> | PullRequestRecord
  isManagedPr(
    owner: string,
    repo: string,
    prNumber: number,
    githubUsername: string,
  ): Promise<boolean> | boolean
  replyToPr(
    token: string,
    input: { owner: string; repo: string; prNumber: number; body: string },
    env?: Env,
  ): Promise<void> | void
  handleGitHubWebhook(event: GitHubWebhookInput): Promise<RepoEvent> | RepoEvent
  resolveEventOwner?(event: RepoEvent): Promise<string | undefined> | string | undefined
  addStreamSocket?(streamKey: string, socket: unknown): void
  removeStreamSocket?(streamKey: string, socket: unknown): void
}

/** HTTP-friendly error type that preserves the intended response status code. */
export class HttpError extends Error {
  constructor(
    readonly statusCode: number,
    message: string,
  ) {
    super(message)
  }
}

/** Validates that a GitHub repository reference contains both owner and repo names. */
export function assertRepo(owner: string, repo: string): void {
  if (!owner?.trim() || !repo?.trim()) {
    throw new HttpError(400, "owner and repo are required")
  }
}

/** Posts a managed PR reply through the configured GitHub App installation. */
export async function postPrCommentViaApp(
  env: Env | undefined,
  owner: string,
  repo: string,
  prNumber: number,
  body: string,
): Promise<void> {
  const octokit = await createInstallationOctokit(env, owner, repo)

  try {
    await octokit.rest.issues.createComment({
      owner,
      repo,
      issue_number: prNumber,
      body,
    })
  } catch (error) {
    throw new HttpError(500, `Failed to post comment to GitHub: ${(error as Error).message}`)
  }
}

/** Creates a pull request through the configured GitHub App and returns its durable identity. */
export async function createPrViaApp(
  env: Env | undefined,
  input: CreatePrInput,
  body: string,
): Promise<{ number: number; url: string; createdAt: string }> {
  const octokit = await createInstallationOctokit(env, input.owner, input.repo)

  try {
    const { data } = await octokit.rest.pulls.create({
      owner: input.owner,
      repo: input.repo,
      title: input.title,
      body,
      head: input.head,
      base: input.base,
    })

    return {
      number: data.number,
      url: data.html_url,
      createdAt: data.created_at,
    }
  } catch (error) {
    throw new HttpError(500, `Failed to create pull request on GitHub: ${(error as Error).message}`)
  }
}

/** Resolves the GitHub App installation that grants backend authority for one repository. */
async function createInstallationOctokit(env: Env | undefined, owner: string, repo: string) {
  if (!env?.GITHUB_APP_ID || !env?.GITHUB_APP_PRIVATE_KEY) {
    throw new HttpError(500, "GitHub App credentials are not configured on the backend")
  }

  const { App } = await import("octokit")
  const app = new App({
    appId: env.GITHUB_APP_ID,
    privateKey: env.GITHUB_APP_PRIVATE_KEY,
  })

  try {
    const { data } = await app.octokit.request("GET /repos/{owner}/{repo}/installation", {
      owner,
      repo,
    })
    return app.getInstallationOctokit(data.id)
  } catch {
    throw new HttpError(500, `Failed to get GitHub App installation for ${owner}/${repo}`)
  }
}
