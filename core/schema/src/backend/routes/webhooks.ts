import { $type, route } from "rouzer"
import { GitHubWebhookInput, type RepoEvent } from "../repo-events.js"

/** Receives normalized GitHub webhook payloads for managed PR feedback. */
export const githubWebhookRoute = route("webhooks/github", {
  POST: {
    body: GitHubWebhookInput,
    response: $type<RepoEvent>(),
  },
})
