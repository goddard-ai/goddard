import type { BackendPrincipal } from "@goddard-ai/auth/schema"
import {
  defineBackendEvents,
  defineBackendEventSources,
  getBackendProviderCapability,
  type BackendEventEnvelope,
  type BackendProviderCapabilityDefinitions,
} from "@goddard-ai/backend-plugin"

import {
  RepoEvent,
  RepoPullRequestCommentCreatedEvent,
  RepoPullRequestCreatedEvent,
  RepoPullRequestReviewSubmittedEvent,
} from "../schema.ts"

export const REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED =
  "remote_repo.pull_request.comment.created" as const
export const REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED =
  "remote_repo.pull_request.review.submitted" as const
export const REMOTE_REPO_PULL_REQUEST_CREATED = "remote_repo.pull_request.created" as const

type RemoteRepoPrincipal = BackendPrincipal

export type RemoteRepoPullRequestCommentCreatedEvent = BackendEventEnvelope<
  typeof REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
  RepoPullRequestCommentCreatedEvent
>

export type RemoteRepoPullRequestReviewSubmittedEvent = BackendEventEnvelope<
  typeof REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
  RepoPullRequestReviewSubmittedEvent
>

export type RemoteRepoPullRequestCreatedEvent = BackendEventEnvelope<
  typeof REMOTE_REPO_PULL_REQUEST_CREATED,
  RepoPullRequestCreatedEvent
>

export type RemoteRepoBackendEvent =
  | RemoteRepoPullRequestCommentCreatedEvent
  | RemoteRepoPullRequestReviewSubmittedEvent
  | RemoteRepoPullRequestCreatedEvent

export const remoteRepoBackendEvents = defineBackendEvents({
  [REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED]: {
    payload: RepoPullRequestCommentCreatedEvent,
  },
  [REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED]: {
    payload: RepoPullRequestReviewSubmittedEvent,
  },
  [REMOTE_REPO_PULL_REQUEST_CREATED]: {
    payload: RepoPullRequestCreatedEvent,
  },
})

export const remoteRepoBackendEventSources = defineBackendEventSources({
  "remote-repo": {
    produces: [
      REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
      REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
      REMOTE_REPO_PULL_REQUEST_CREATED,
    ],
    authorize: ({
      principal,
      event,
      providers,
    }: {
      principal: RemoteRepoPrincipal
      event: RemoteRepoBackendEvent
      providers: BackendProviderCapabilityDefinitions
    }) => {
      if (event.name === REMOTE_REPO_PULL_REQUEST_CREATED) {
        return principalHasDisplayName(principal, event.payload.author)
      }

      const provider = getProviderCapability(providers, event.payload.provider)
      return (
        provider?.authorizeRemoteRepositoryAccess?.({
          principal,
          repository: event.payload,
        }) ?? false
      )
    },
  },
})

export function createRemoteRepoBackendEvent(event: RepoEvent): RemoteRepoBackendEvent {
  switch (event.type) {
    case "comment":
      return {
        name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
        payload: event,
      }
    case "review":
      return {
        name: REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
        payload: event,
      }
    case "pr.created":
      return {
        name: REMOTE_REPO_PULL_REQUEST_CREATED,
        payload: event,
      }
  }
}

function principalHasDisplayName(principal: RemoteRepoPrincipal, displayName: string) {
  return principal.providerIdentities.some(
    (identity) => identity.displayName === displayName || identity.subject === displayName,
  )
}

function getProviderCapability(providers: BackendProviderCapabilityDefinitions, provider: string) {
  try {
    return getBackendProviderCapability(providers, provider)
  } catch {
    return undefined
  }
}

/** Feature-owned backend handler for normalized remote repository events. */
export type RemoteRepoEventHandler = {
  name: string
  canHandle?: (event: RepoEvent) => boolean
  handle: (event: RepoEvent) => Promise<void> | void
}

/** Preserves the handler object while constraining it to the remote-repo event contract. */
export function defineRemoteRepoEventHandler<const THandler extends RemoteRepoEventHandler>(
  handler: THandler,
) {
  return handler
}

/** Dispatches one normalized remote repository event to interested feature handlers. */
export async function dispatchRemoteRepoEvent(
  event: RepoEvent,
  handlers: readonly RemoteRepoEventHandler[],
) {
  for (const handler of handlers) {
    if (handler.canHandle && !handler.canHandle(event)) {
      continue
    }

    await handler.handle(event)
  }
}
