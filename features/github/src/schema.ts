import { z } from "zod"

export const GitHubRepositoryRef = z.strictObject({
  owner: z.string().min(1),
  repo: z.string().min(1),
})

export type GitHubRepositoryRef = z.infer<typeof GitHubRepositoryRef>

export const GitHubUserPrincipal = z.strictObject({
  kind: z.literal("github_user"),
  githubUserId: z.number().int().positive(),
  githubLogin: z.string().min(1),
  repositories: z.array(GitHubRepositoryRef).optional(),
})

export type GitHubUserPrincipal = z.infer<typeof GitHubUserPrincipal>

const githubIssueCommentWebhook = GitHubRepositoryRef.extend({
  type: z.literal("issue_comment"),
  prNumber: z.number().int().positive(),
  author: z.string().min(1),
  body: z.string(),
})

const githubPullRequestReviewWebhook = GitHubRepositoryRef.extend({
  type: z.literal("pull_request_review"),
  prNumber: z.number().int().positive(),
  author: z.string().min(1),
  state: z.enum(["approved", "changes_requested", "commented"]),
  body: z.string(),
})

export const GitHubWebhookInput = z.discriminatedUnion("type", [
  githubIssueCommentWebhook,
  githubPullRequestReviewWebhook,
])

export type GitHubWebhookInput = z.infer<typeof GitHubWebhookInput>

export const GitHubWebhookDeliveryInput = z.strictObject({
  deliveryId: z.string().min(1),
  receivedAt: z.string().optional(),
  event: GitHubWebhookInput,
})

export type GitHubWebhookDeliveryInput = z.infer<typeof GitHubWebhookDeliveryInput>

export const GitHubEventProvenance = z.strictObject({
  provider: z.literal("github"),
  deliveryId: z.string().min(1),
  webhookType: z.string().min(1),
})

export type GitHubEventProvenance = z.infer<typeof GitHubEventProvenance>
