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

export const GitHubEventProvenance = z.strictObject({
  provider: z.literal("github"),
  deliveryId: z.string().min(1),
  webhookType: z.string().min(1),
})

export type GitHubEventProvenance = z.infer<typeof GitHubEventProvenance>
