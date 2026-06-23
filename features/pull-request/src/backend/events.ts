import { defineBackendEventSources, type BackendEventEnvelope } from "@goddard-ai/backend-plugin"
import { defineRemoteRepoEventHandler } from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"

type PullRequestRepoEvent = Extract<RepoEvent, { type: "comment" | "review" | "pr.created" }>
type PullRequestPrincipal = {
  readonly githubLogin: string
  readonly repositories?: readonly { readonly owner: string; readonly repo: string }[]
}
type PullRequestBackendEvent = BackendEventEnvelope<"remote_repo.event.received", RepoEvent>

function isPullRequestRepoEvent(event: RepoEvent): event is PullRequestRepoEvent {
  return event.type === "comment" || event.type === "review" || event.type === "pr.created"
}

/** Pull-request backend handler for PR-shaped remote repository events. */
export const pullRequestRemoteRepoEventHandler = defineRemoteRepoEventHandler({
  name: "pull-request",
  canHandle: isPullRequestRepoEvent,
  handle: (_event) => {},
})

export const pullRequestBackendEventSources = defineBackendEventSources({
  "pull-request": {
    produces: ["remote_repo.event.received"],
    authorize: ({
      principal,
      event,
    }: {
      principal: PullRequestPrincipal
      event: PullRequestBackendEvent
    }) => {
      if (event.payload.type === "pr.created") {
        return event.payload.author === principal.githubLogin
      }

      return (
        principal.repositories?.some(
          (repository) =>
            repository.owner === event.payload.owner && repository.repo === event.payload.repo,
        ) ?? false
      )
    },
  },
})
