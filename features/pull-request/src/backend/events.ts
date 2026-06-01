import { defineRemoteRepoEventHandler } from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"

type PullRequestRepoEvent = Extract<RepoEvent, { type: "comment" | "review" | "pr.created" }>

function isPullRequestRepoEvent(event: RepoEvent): event is PullRequestRepoEvent {
  return event.type === "comment" || event.type === "review" || event.type === "pr.created"
}

/** Pull-request backend handler for PR-shaped remote repository events. */
export const pullRequestRemoteRepoEventHandler = defineRemoteRepoEventHandler({
  name: "pull-request",
  canHandle: isPullRequestRepoEvent,
  handle: (_event) => {},
})
