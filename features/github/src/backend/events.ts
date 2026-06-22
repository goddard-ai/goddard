import { defineBackendEvents, type BackendEventEnvelope } from "@goddard-ai/backend-plugin"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"

import type {
  GitHubEventProvenance,
  GitHubRepositoryRef,
  GitHubUserPrincipal,
  GitHubWebhookDeliveryInput,
  GitHubWebhookInput,
} from "../schema.ts"

export type GitHubRemoteRepoEvent = BackendEventEnvelope<
  "remote_repo.event.received",
  RepoEvent,
  GitHubEventProvenance
>

export function normalizeGitHubWebhookEvent(
  event: GitHubWebhookInput,
  createdAt = new Date().toISOString(),
): RepoEvent {
  if (event.type === "issue_comment") {
    return {
      type: "comment",
      owner: event.owner,
      repo: event.repo,
      prNumber: event.prNumber,
      author: event.author,
      body: event.body,
      reactionAdded: "eyes",
      createdAt,
    }
  }

  return {
    type: "review",
    owner: event.owner,
    repo: event.repo,
    prNumber: event.prNumber,
    author: event.author,
    state: event.state,
    body: event.body,
    reactionAdded: "eyes",
    createdAt,
  }
}

export function normalizeGitHubWebhookDelivery(
  delivery: GitHubWebhookDeliveryInput,
): GitHubRemoteRepoEvent {
  return {
    name: "remote_repo.event.received",
    payload: normalizeGitHubWebhookEvent(delivery.event, delivery.receivedAt),
    provenance: {
      provider: "github",
      deliveryId: delivery.deliveryId,
      webhookType: delivery.event.type,
    },
  }
}

export function canGitHubPrincipalAccessRepository(
  principal: GitHubUserPrincipal,
  repository: GitHubRepositoryRef,
) {
  if (!principal.repositories) {
    return false
  }

  return principal.repositories.some(
    (allowed) => allowed.owner === repository.owner && allowed.repo === repository.repo,
  )
}

export const githubBackendEvents = defineBackendEvents({
  "remote_repo.event.received": {
    normalizeWebhook: normalizeGitHubWebhookDelivery,
    authorize: ({ principal, event }) =>
      canGitHubPrincipalAccessRepository(principal, event.payload),
    matchesFilter: ({ event, filter }) => {
      if (!filter || typeof filter !== "object") {
        return true
      }

      const repo = filter as Partial<GitHubRepositoryRef>
      return (
        (repo.owner === undefined || repo.owner === event.payload.owner) &&
        (repo.repo === undefined || repo.repo === event.payload.repo)
      )
    },
  },
})
