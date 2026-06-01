import { z } from "zod"

/** Repository identity for a remotely hosted source repository. */
export const RemoteRepositoryRef = z.object({
  owner: z.string(),
  repo: z.string(),
})

export type RemoteRepositoryRef = z.infer<typeof RemoteRepositoryRef>

/** Repository owner and name tuple used by remote repository contracts. */
export const RepoRef = RemoteRepositoryRef

export type RepoRef = z.infer<typeof RepoRef>

const repoCommentEvent = RemoteRepositoryRef.extend({
  type: z.literal("comment"),
  prNumber: z.number(),
  author: z.string(),
  body: z.string(),
  reactionAdded: z.literal("eyes"),
  createdAt: z.string(),
})

const repoReviewEvent = RemoteRepositoryRef.extend({
  type: z.literal("review"),
  prNumber: z.number(),
  author: z.string(),
  state: z.enum(["approved", "changes_requested", "commented"]),
  body: z.string(),
  reactionAdded: z.literal("eyes"),
  createdAt: z.string(),
})

const repoPullRequestCreatedEvent = RemoteRepositoryRef.extend({
  type: z.literal("pr.created"),
  prNumber: z.number(),
  title: z.string(),
  author: z.string(),
  createdAt: z.string(),
})

/** Normalized remote repository activity event emitted by backend workflows. */
export const RepoEvent = z.discriminatedUnion("type", [
  repoCommentEvent,
  repoReviewEvent,
  repoPullRequestCreatedEvent,
])

export type RepoEvent = z.infer<typeof RepoEvent>

/** SSE payload delivered over the backend remote repository stream. */
export const StreamMessage = z.object({
  event: RepoEvent,
})

export type StreamMessage = z.infer<typeof StreamMessage>
