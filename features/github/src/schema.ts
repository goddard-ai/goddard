import { BackendPrincipal, ProviderIdentity } from "@goddard-ai/auth/schema"
import { z } from "zod"

export const GitHubRepositoryRef = z.strictObject({
  owner: z.string().min(1),
  repo: z.string().min(1),
})

export type GitHubRepositoryRef = z.infer<typeof GitHubRepositoryRef>

export const GitHubIdentity = z.strictObject({
  provider: z.literal("github"),
  githubUserId: z.number().int().positive(),
  githubLogin: z.string().min(1),
})

export type GitHubIdentity = z.infer<typeof GitHubIdentity>

export const GitHubProviderIdentity = ProviderIdentity.extend({
  provider: z.literal("github"),
})

export type GitHubProviderIdentity = z.infer<typeof GitHubProviderIdentity>

export const GitHubRepositoryGrant = GitHubRepositoryRef.extend({
  provider: z.literal("github"),
})

export type GitHubRepositoryGrant = z.infer<typeof GitHubRepositoryGrant>

export const GitHubPrincipalGrants = z.strictObject({
  identity: GitHubIdentity,
  repositories: z.array(GitHubRepositoryRef).optional(),
})

export type GitHubPrincipalGrants = z.infer<typeof GitHubPrincipalGrants>

export const GitHubEventProvenance = z.strictObject({
  provider: z.literal("github"),
  deliveryId: z.string().min(1),
  webhookType: z.string().min(1),
})

export type GitHubEventProvenance = z.infer<typeof GitHubEventProvenance>

export function createGitHubProviderIdentity(identity: GitHubIdentity): GitHubProviderIdentity {
  return GitHubProviderIdentity.parse({
    provider: "github",
    subject: String(identity.githubUserId),
    displayName: identity.githubLogin,
  })
}

export function createGitHubBackendPrincipal(identity: GitHubIdentity): BackendPrincipal {
  return BackendPrincipal.parse({
    id: `github:${identity.githubUserId}`,
    providerIdentities: [createGitHubProviderIdentity(identity)],
  })
}

export function canGitHubPrincipalAccessRepository(
  grants: GitHubPrincipalGrants,
  repository: GitHubRepositoryRef,
) {
  return (
    grants.repositories?.some(
      (allowed) => allowed.owner === repository.owner && allowed.repo === repository.repo,
    ) ?? false
  )
}
