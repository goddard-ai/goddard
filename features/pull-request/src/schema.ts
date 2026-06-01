import { SessionInboxMetadataInput } from "@goddard-ai/inbox/schema"
import { DaemonPullRequestId, DaemonPullRequestIdParams } from "@goddard-ai/schema/id"
import { RepoPrRef, RepoRef } from "@goddard-ai/schema/repository"
import { z } from "zod"

/** Request payload used to create one managed pull request. */
export const CreatePrInput = RepoRef.extend({
  title: z.string(),
  body: z.string().optional(),
  head: z.string(),
  base: z.string(),
})

export type CreatePrInput = z.infer<typeof CreatePrInput>

/** Request payload used to reply to one managed pull request. */
export const ReplyPrInput = RepoPrRef.extend({
  body: z.string(),
})

export type ReplyPrInput = z.infer<typeof ReplyPrInput>

/** Query payload used to check whether one pull request is managed. */
export const ManagedPrQuery = RepoRef.extend({
  prNumber: z.coerce.number(),
})

export type ManagedPrQuery = z.infer<typeof ManagedPrQuery>

/** Persistent backend record describing one managed pull request. */
export const PullRequestRecord = z.object({
  id: z.number(),
  number: z.number(),
  owner: z.string(),
  repo: z.string(),
  title: z.string(),
  body: z.string(),
  head: z.string(),
  base: z.string(),
  url: z.string(),
  createdBy: z.string(),
  createdAt: z.string(),
})

export type PullRequestRecord = z.infer<typeof PullRequestRecord>

const repoCommentEvent = z.object({
  type: z.literal("comment"),
  owner: z.string(),
  repo: z.string(),
  prNumber: z.number(),
  author: z.string(),
  body: z.string(),
  reactionAdded: z.literal("eyes"),
  createdAt: z.string(),
})

const repoReviewEvent = z.object({
  type: z.literal("review"),
  owner: z.string(),
  repo: z.string(),
  prNumber: z.number(),
  author: z.string(),
  state: z.enum(["approved", "changes_requested", "commented"]),
  body: z.string(),
  reactionAdded: z.literal("eyes"),
  createdAt: z.string(),
})

const repoPullRequestCreatedEvent = z.object({
  type: z.literal("pr.created"),
  owner: z.string(),
  repo: z.string(),
  prNumber: z.number(),
  title: z.string(),
  author: z.string(),
  createdAt: z.string(),
})

/** Normalized repository activity event emitted by backend workflows. */
export const RepoEvent = z.discriminatedUnion("type", [
  repoCommentEvent,
  repoReviewEvent,
  repoPullRequestCreatedEvent,
])

export type RepoEvent = z.infer<typeof RepoEvent>

/** SSE payload delivered over the backend feedback stream. */
export const StreamMessage = z.object({
  event: RepoEvent,
})

export type StreamMessage = z.infer<typeof StreamMessage>

/** Persisted daemon-managed pull request record owned by the pull-request feature. */
export const DaemonPullRequest = z.strictObject({
  host: z.enum(["github"]),
  owner: z.string(),
  repo: z.string(),
  prNumber: z.number().int(),
  cwd: z.string(),
})

export type DaemonPullRequest = z.output<typeof DaemonPullRequest> & {
  id: DaemonPullRequestId
  updatedAt: number
}

const issueCommentWebhook = z.object({
  type: z.literal("issue_comment"),
  owner: z.string(),
  repo: z.string(),
  prNumber: z.number(),
  author: z.string(),
  body: z.string(),
})

const pullRequestReviewWebhook = z.object({
  type: z.literal("pull_request_review"),
  owner: z.string(),
  repo: z.string(),
  prNumber: z.number(),
  author: z.string(),
  state: z.enum(["approved", "changes_requested", "commented"]),
  body: z.string(),
})

/** Normalized GitHub webhook payload accepted by backend webhook handlers. */
export const GitHubWebhookInput = z.discriminatedUnion("type", [
  issueCommentWebhook,
  pullRequestReviewWebhook,
])

export type GitHubWebhookInput = z.infer<typeof GitHubWebhookInput>

/** Request payload used to create one pull request through the daemon. */
export const SubmitPrRequest = z.strictObject({
  cwd: z.string(),
  title: z.string(),
  body: z.string(),
  head: z.string().optional(),
  base: z.string().optional(),
  scope: SessionInboxMetadataInput.shape.scope,
  headline: SessionInboxMetadataInput.shape.headline,
})

export type SubmitPrRequest = z.infer<typeof SubmitPrRequest>

/** Response payload returned after one pull request submission. */
export type SubmitPrResponse = {
  number: number
  url: string
}

/** Request payload used to reply to one pull request through the daemon. */
export const ReplyPrRequest = z.strictObject({
  cwd: z.string(),
  message: z.string(),
  prNumber: z.number().optional(),
  scope: SessionInboxMetadataInput.shape.scope,
  headline: SessionInboxMetadataInput.shape.headline,
})

export type ReplyPrRequest = z.infer<typeof ReplyPrRequest>

/** Response payload returned after one pull request reply. */
export type ReplyPrResponse = {
  success: boolean
}

/** Request payload used to fetch one stored daemon pull request by tagged id. */
export const GetPullRequestRequest = DaemonPullRequestIdParams

export type GetPullRequestRequest = z.infer<typeof GetPullRequestRequest>

/** Response payload returned after fetching one stored daemon pull request. */
export type GetPullRequestResponse = {
  pullRequest: DaemonPullRequest
}
