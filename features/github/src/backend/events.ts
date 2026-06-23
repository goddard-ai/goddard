import {
  createRemoteRepoBackendEvent,
  type RemoteRepoBackendEvent,
} from "@goddard-ai/remote-repo/backend"
import { z } from "zod"

import type { GitHubEventProvenance } from "../schema.ts"

export type GitHubRemoteRepoEvent = RemoteRepoBackendEvent & {
  readonly provenance: GitHubEventProvenance
}

export type GitHubWebhookRequestInput = {
  deliveryId: string
  eventName: string
  payload: unknown
  receivedAt?: string
}

export class GitHubWebhookError extends Error {
  constructor(
    readonly statusCode: number,
    message: string,
  ) {
    super(message)
  }
}

export async function readGitHubWebhookRequest(
  request: Request,
  webhookSecret?: string,
): Promise<GitHubWebhookRequestInput> {
  const body = await request.text()
  if (webhookSecret) {
    await assertGitHubWebhookSignature(request, webhookSecret, body)
  }

  const eventName = request.headers.get("x-github-event")
  if (!eventName) {
    throw new GitHubWebhookError(400, "Missing GitHub webhook event")
  }

  const deliveryId = request.headers.get("x-github-delivery")
  if (!deliveryId) {
    throw new GitHubWebhookError(400, "Missing GitHub webhook delivery")
  }

  return {
    deliveryId,
    eventName,
    payload: JSON.parse(body) as unknown,
  }
}

const githubUser = z.object({
  login: z.string().min(1),
  type: z.string().optional(),
})

const githubRepository = z.object({
  name: z.string().min(1),
  owner: z.object({
    login: z.string().min(1),
  }),
})

const issueCommentPayload = z.object({
  action: z.literal("created"),
  issue: z.object({
    number: z.number().int().positive(),
    pull_request: z.unknown().optional(),
  }),
  comment: z.object({
    body: z.string().nullable().optional(),
    user: githubUser.nullable().optional(),
  }),
  repository: githubRepository,
  sender: githubUser.nullable().optional(),
})

const pullRequestReviewPayload = z.object({
  action: z.literal("submitted"),
  pull_request: z.object({
    number: z.number().int().positive(),
  }),
  review: z.object({
    body: z.string().nullable().optional(),
    state: z.string(),
    user: githubUser.nullable().optional(),
  }),
  repository: githubRepository,
  sender: githubUser.nullable().optional(),
})

export function normalizeGitHubWebhookRequest(
  input: GitHubWebhookRequestInput,
): GitHubRemoteRepoEvent | undefined {
  const createdAt = input.receivedAt ?? new Date().toISOString()

  if (input.eventName === "issue_comment") {
    const payload = issueCommentPayload.parse(input.payload)
    if (!payload.issue.pull_request || isBot(payload.sender) || isBot(payload.comment.user)) {
      return undefined
    }

    return withGitHubProvenance(
      createRemoteRepoBackendEvent({
        type: "comment",
        owner: payload.repository.owner.login,
        repo: payload.repository.name,
        prNumber: payload.issue.number,
        author: payload.comment.user?.login ?? "unknown",
        body: payload.comment.body ?? "",
        reactionAdded: "eyes",
        createdAt,
      }),
      {
        provider: "github",
        deliveryId: input.deliveryId,
        webhookType: input.eventName,
      },
    )
  }

  if (input.eventName === "pull_request_review") {
    const payload = pullRequestReviewPayload.parse(input.payload)
    const state = payload.review.state.toLowerCase()
    if (
      !["approved", "changes_requested", "commented"].includes(state) ||
      isBot(payload.sender) ||
      isBot(payload.review.user)
    ) {
      return undefined
    }

    return withGitHubProvenance(
      createRemoteRepoBackendEvent({
        type: "review",
        owner: payload.repository.owner.login,
        repo: payload.repository.name,
        prNumber: payload.pull_request.number,
        author: payload.review.user?.login ?? "unknown",
        state: state as "approved" | "changes_requested" | "commented",
        body: payload.review.body ?? "",
        reactionAdded: "eyes",
        createdAt,
      }),
      {
        provider: "github",
        deliveryId: input.deliveryId,
        webhookType: input.eventName,
      },
    )
  }

  return undefined
}

function isBot(user: { type?: string } | null | undefined) {
  return user?.type === "Bot"
}

function withGitHubProvenance(
  event: RemoteRepoBackendEvent,
  provenance: GitHubEventProvenance,
): GitHubRemoteRepoEvent {
  return {
    ...event,
    provenance,
  }
}

async function assertGitHubWebhookSignature(request: Request, webhookSecret: string, body: string) {
  const signature = request.headers.get("x-hub-signature-256")
  if (!signature?.startsWith("sha256=")) {
    throw new GitHubWebhookError(401, "Missing GitHub webhook signature")
  }

  const expected = await signGitHubWebhookBody(webhookSecret, body)
  if (!constantTimeEqual(signature, expected)) {
    throw new GitHubWebhookError(401, "Invalid GitHub webhook signature")
  }
}

export async function signGitHubWebhookBody(secret: string, body: string): Promise<string> {
  const encoder = new TextEncoder()
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  )
  const signature = await crypto.subtle.sign("HMAC", key, encoder.encode(body))
  const hex = [...new Uint8Array(signature)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("")
  return `sha256=${hex}`
}

function constantTimeEqual(left: string, right: string): boolean {
  if (left.length !== right.length) {
    return false
  }

  let difference = 0
  for (let index = 0; index < left.length; index += 1) {
    difference |= left.charCodeAt(index) ^ right.charCodeAt(index)
  }

  return difference === 0
}
