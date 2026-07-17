import type {
  AuthSession,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
} from "@goddard-ai/auth/schema"
import {
  getBackendProviderCapability,
  type BackendProviderCapabilityDefinitions,
} from "@goddard-ai/backend-plugin"
import type {
  CreatePrInput,
  PullRequestRecord,
  ReplyPrInput,
} from "@goddard-ai/pull-request/schema"

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
    provider: string,
    owner: string,
    repo: string,
    prNumber: number,
    principalId: string,
  ): Promise<boolean> | boolean
  replyToPr(token: string, input: ReplyPrInput, env?: Env): Promise<void> | void
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

/** Validates that a repository reference contains both owner and repo names. */
export function assertRepo(owner: string, repo: string): void {
  if (!owner?.trim() || !repo?.trim()) {
    throw new HttpError(400, "owner and repo are required")
  }
}

/** Posts a managed PR reply through the composed provider capability. */
export async function postPrCommentViaProvider(
  env: Env | undefined,
  providers: BackendProviderCapabilityDefinitions,
  input: ReplyPrInput,
): Promise<void> {
  try {
    const provider = getBackendProviderCapability(providers, input.provider)
    if (!provider.createPullRequestComment) {
      throw new HttpError(
        500,
        `Backend provider cannot comment on pull requests: ${input.provider}`,
      )
    }

    await provider.createPullRequestComment({
      env,
      ...input,
    })
  } catch (error) {
    throw toHttpError(error)
  }
}

/** Creates a pull request through the composed provider capability. */
export async function createPrViaProvider(
  env: Env | undefined,
  providers: BackendProviderCapabilityDefinitions,
  input: CreatePrInput,
  body: string,
): Promise<{ number: number; url: string; createdAt: string }> {
  try {
    const provider = getBackendProviderCapability(providers, input.provider)
    if (!provider.createPullRequest) {
      throw new HttpError(500, `Backend provider cannot create pull requests: ${input.provider}`)
    }

    const result = await provider.createPullRequest({
      env,
      ...input,
      body,
    })

    return {
      number: result.number,
      url: result.url,
      createdAt: result.createdAt ?? new Date().toISOString(),
    }
  } catch (error) {
    throw toHttpError(error)
  }
}

function toHttpError(error: unknown): HttpError {
  if (error instanceof HttpError) {
    return error
  }
  if (error && typeof error === "object" && "statusCode" in error && "message" in error) {
    const statusCode = Number((error as { statusCode: unknown }).statusCode)
    const message = String((error as { message: unknown }).message)
    return new HttpError(Number.isFinite(statusCode) ? statusCode : 500, message)
  }
  return new HttpError(500, error instanceof Error ? error.message : String(error))
}
