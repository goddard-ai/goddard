import { z } from "zod"

/** Repository identity for a remotely hosted source repository. */
export const RemoteRepositoryRef = z.object({
  provider: z.string().min(1),
  owner: z.string(),
  repo: z.string(),
})

export type RemoteRepositoryRef = z.infer<typeof RemoteRepositoryRef>

/** Repository owner and name tuple used by remote repository contracts. */
export const RepoRef = RemoteRepositoryRef

export type RepoRef = z.infer<typeof RepoRef>

export const RepoPullRequestCommentCreatedEvent = RemoteRepositoryRef.extend({
  type: z.literal("comment"),
  prNumber: z.number(),
  author: z.string(),
  body: z.string(),
  reactionAdded: z.literal("eyes"),
  createdAt: z.string(),
})

export const RepoPullRequestReviewSubmittedEvent = RemoteRepositoryRef.extend({
  type: z.literal("review"),
  prNumber: z.number(),
  author: z.string(),
  state: z.enum(["approved", "changes_requested", "commented"]),
  body: z.string(),
  reactionAdded: z.literal("eyes"),
  createdAt: z.string(),
})

export const RepoPullRequestCreatedEvent = RemoteRepositoryRef.extend({
  type: z.literal("pr.created"),
  prNumber: z.number(),
  title: z.string(),
  author: z.string(),
  createdAt: z.string(),
})

/** Normalized remote repository activity event emitted by backend workflows. */
export const RepoEvent = z.discriminatedUnion("type", [
  RepoPullRequestCommentCreatedEvent,
  RepoPullRequestReviewSubmittedEvent,
  RepoPullRequestCreatedEvent,
])

export type RepoEvent = z.infer<typeof RepoEvent>
export type RepoPullRequestCommentCreatedEvent = z.infer<typeof RepoPullRequestCommentCreatedEvent>
export type RepoPullRequestReviewSubmittedEvent = z.infer<
  typeof RepoPullRequestReviewSubmittedEvent
>
export type RepoPullRequestCreatedEvent = z.infer<typeof RepoPullRequestCreatedEvent>

export const RepoEventName = z.enum(["comment", "review", "pr.created"])

export type RepoEventName = z.infer<typeof RepoEventName>

export const RepoEventFilter = z.object({
  owner: z.string().optional(),
  repo: z.string().optional(),
  prNumber: z.number().optional(),
})

export type RepoEventFilter = z.infer<typeof RepoEventFilter>

/** Request body accepted by the backend remote repository event stream. */
export const BackendEventStreamRequest = z.object({
  names: z.array(RepoEventName).optional(),
  where: RepoEventFilter.optional(),
})

export type BackendEventStreamRequest = z.infer<typeof BackendEventStreamRequest>

const issueCommentWebhook = RemoteRepositoryRef.extend({
  type: z.literal("issue_comment"),
  prNumber: z.number(),
  author: z.string(),
  body: z.string(),
})

const pullRequestReviewWebhook = RemoteRepositoryRef.extend({
  type: z.literal("pull_request_review"),
  prNumber: z.number(),
  author: z.string(),
  state: z.enum(["approved", "changes_requested", "commented"]),
  body: z.string(),
})

/** Normalized GitHub webhook payload accepted by remote-repo backend webhook handlers. */
export const GitHubWebhookInput = z.discriminatedUnion("type", [
  issueCommentWebhook,
  pullRequestReviewWebhook,
])

export type GitHubWebhookInput = z.infer<typeof GitHubWebhookInput>
