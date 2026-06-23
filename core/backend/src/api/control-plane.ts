import type {
  AuthSession,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
} from "@goddard-ai/auth/schema"
import {
  createPullRequestCommentWithGitHubApp,
  createPullRequestWithGitHubApp,
  GitHubProviderError,
} from "@goddard-ai/github/backend"
import type { CreatePrInput, PullRequestRecord } from "@goddard-ai/pull-request/schema"

import type { Env } from "../env.ts"
import type { BackendPrincipal } from "./events.ts"

/** Backend operations that the HTTP router can delegate to a storage implementation. */
export interface BackendControlPlane {
  startDeviceFlow(input?: DeviceFlowStart): Promise<DeviceFlowSession> | DeviceFlowSession
  completeDeviceFlow(input: DeviceFlowComplete): Promise<AuthSession> | AuthSession
  getSession(token: string): Promise<AuthSession> | AuthSession
  getPrincipal(token: string): Promise<BackendPrincipal> | BackendPrincipal
  createPr(
    token: string,
    input: CreatePrInput,
    env?: Env,
  ): Promise<PullRequestRecord> | PullRequestRecord
  isManagedPr(
    owner: string,
    repo: string,
    prNumber: number,
    principalId: string,
  ): Promise<boolean> | boolean
  replyToPr(
    token: string,
    input: { owner: string; repo: string; prNumber: number; body: string },
    env?: Env,
  ): Promise<void> | void
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
  try {
    await createPullRequestCommentWithGitHubApp({
      env,
      provider: "github",
      owner,
      repo,
      prNumber,
      body,
    })
  } catch (error) {
    throw toHttpError(error)
  }
}

/** Creates a pull request through the configured GitHub App and returns its durable identity. */
export async function createPrViaApp(
  env: Env | undefined,
  input: CreatePrInput,
  body: string,
): Promise<{ number: number; url: string; createdAt: string }> {
  try {
    return await createPullRequestWithGitHubApp({
      env,
      provider: "github",
      owner: input.owner,
      repo: input.repo,
      title: input.title,
      body,
      head: input.head,
      base: input.base,
    })
  } catch (error) {
    throw toHttpError(error)
  }
}

function toHttpError(error: unknown): HttpError {
  if (error instanceof HttpError) {
    return error
  }
  if (error instanceof GitHubProviderError) {
    return new HttpError(error.statusCode, error.message)
  }
  return new HttpError(500, error instanceof Error ? error.message : String(error))
}
