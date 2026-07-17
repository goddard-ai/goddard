import type { BackendPrincipal as AuthBackendPrincipal, AuthSession } from "@goddard-ai/auth/schema"
import type { BackendProviderCapabilityDefinitions } from "@goddard-ai/backend-plugin"
import {
  remoteRepoBackendEventSources,
  type RemoteRepoBackendEvent,
} from "@goddard-ai/remote-repo/backend"
import type { RemoteRepositoryRef, RepoEvent } from "@goddard-ai/remote-repo/schema"

/** Authenticated backend stream principal resolved from a backend session token. */
export type BackendPrincipal = AuthBackendPrincipal & {
  readonly repositories?: readonly RemoteRepositoryRef[]
}

/** Returns the stable durable stream key for a backend principal. */
export function getPrincipalStreamKey(principal: BackendPrincipal): string {
  return principal.id
}

/** Converts a public auth session into the backend principal shape used for event auth. */
export function sessionToPrincipal(
  session: AuthSession,
  repositories?: readonly RemoteRepositoryRef[],
): BackendPrincipal {
  return {
    ...session.principal,
    repositories: repositories ? [...repositories] : undefined,
  }
}

/** Returns the best user-facing identity label for local PR text and tests. */
export function getPrincipalDisplayName(principal: AuthBackendPrincipal): string {
  return (
    principal.providerIdentities.find((identity) => identity.displayName)?.displayName ??
    principal.providerIdentities[0]?.subject ??
    principal.id
  )
}

/** Returns the repository identity carried by a normalized remote-repo event. */
export function getRepoEventRepository(event: RepoEvent): RemoteRepositoryRef {
  return {
    provider: event.provider,
    owner: event.owner,
    repo: event.repo,
  }
}

/** Applies the remote-repo feature's backend event authorization contract. */
export async function authorizeRemoteRepoBackendEvent(
  principal: BackendPrincipal,
  event: RemoteRepoBackendEvent,
  providers: BackendProviderCapabilityDefinitions,
): Promise<boolean> {
  return remoteRepoBackendEventSources["remote-repo"].authorize({
    principal,
    event,
    providers,
  })
}
