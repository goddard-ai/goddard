import type { GitHubWebhookInput, RepoEvent } from "../schema.ts"

/** Converts a supported GitHub webhook payload into the remote repository event stream shape. */
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
