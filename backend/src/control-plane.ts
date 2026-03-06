import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PullRequestRecord,
  RepoEvent,
} from "@goddard-ai/schema"

export interface BackendControlPlane {
  startDeviceFlow(input?: DeviceFlowStart): Promise<DeviceFlowSession> | DeviceFlowSession
  completeDeviceFlow(input: DeviceFlowComplete): Promise<AuthSession> | AuthSession
  getSession(token: string): Promise<AuthSession> | AuthSession
  createPr(token: string, input: CreatePrInput): Promise<PullRequestRecord> | PullRequestRecord
  isManagedPr(
    owner: string,
    repo: string,
    prNumber: number,
    githubUsername: string,
  ): Promise<boolean> | boolean
  replyToPr(
    token: string,
    input: { owner: string; repo: string; prNumber: number; body: string },
    env?: any,
  ): Promise<void> | void
  handleGitHubWebhook(event: GitHubWebhookInput): Promise<RepoEvent> | RepoEvent
  addStreamSocket?(repoKey: string, socket: unknown): void
  removeStreamSocket?(repoKey: string, socket: unknown): void
}

export class HttpError extends Error {
  constructor(
    readonly statusCode: number,
    message: string,
  ) {
    super(message)
  }
}

import type { Env } from "./env.ts"

export function assertRepo(owner: string, repo: string): void {
  if (!owner?.trim() || !repo?.trim()) {
    throw new HttpError(400, "owner and repo are required")
  }
}

export async function postPrCommentViaApp(
  env: Env | undefined,
  owner: string,
  repo: string,
  prNumber: number,
  body: string,
): Promise<void> {
  if (!env?.GITHUB_APP_ID || !env?.GITHUB_APP_PRIVATE_KEY) {
    throw new HttpError(500, "GitHub App credentials are not configured on the backend")
  }

  const { App } = await import("octokit")
  const app = new App({
    appId: env.GITHUB_APP_ID,
    privateKey: env.GITHUB_APP_PRIVATE_KEY,
  })

  let installationId: number
  try {
    const { data } = await app.octokit.request("GET /repos/{owner}/{repo}/installation", {
      owner,
      repo,
    })
    installationId = data.id
  } catch {
    throw new HttpError(500, `Failed to get GitHub App installation for ${owner}/${repo}`)
  }

  const octokit = await app.getInstallationOctokit(installationId)

  try {
    await octokit.rest.issues.createComment({
      owner,
      repo,
      issue_number: prNumber,
      body,
    })
  } catch (e: any) {
    throw new HttpError(500, `Failed to post comment to GitHub: ${e.message}`)
  }
}
