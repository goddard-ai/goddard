import type { AuthSession } from "@goddard-ai/auth/schema"
import type { GitHubRepositoryRef, GitHubUserPrincipal } from "@goddard-ai/github/schema"
import {
  remoteRepoBackendEventSources,
  type RemoteRepoBackendEvent,
} from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"

/** Authenticated backend stream principal resolved from a backend session token. */
export type BackendPrincipal = GitHubUserPrincipal

/** Returns the stable durable stream key for a backend principal. */
export function getPrincipalStreamKey(principal: BackendPrincipal): string {
  return `github-user:${principal.githubUserId}`
}

/** Converts a public auth session into the backend principal shape used for event auth. */
export function sessionToPrincipal(
  session: AuthSession,
  repositories?: readonly GitHubRepositoryRef[],
): BackendPrincipal {
  return {
    kind: "github_user",
    githubUserId: session.githubUserId,
    githubLogin: session.githubUsername,
    repositories: repositories ? [...repositories] : undefined,
  }
}

/** Returns the repository identity carried by a normalized remote-repo event. */
export function getRepoEventRepository(event: RepoEvent): GitHubRepositoryRef {
  return {
    owner: event.owner,
    repo: event.repo,
  }
}

/** Applies the remote-repo feature's backend event authorization contract. */
export async function authorizeRemoteRepoBackendEvent(
  principal: BackendPrincipal,
  event: RemoteRepoBackendEvent,
): Promise<boolean> {
  return remoteRepoBackendEventSources["remote-repo"].authorize({
    principal,
    event,
  })
}
