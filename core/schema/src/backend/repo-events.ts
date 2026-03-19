import { z } from "zod"

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
