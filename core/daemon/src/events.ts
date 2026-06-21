import { event } from "@goddard-ai/daemon-plugin"

export type RepoFeedbackEventPayload = {
  repository: string
  owner: string
  repo: string
  prNumber: number
  feedbackType: "comment" | "review"
}

export type RepoFeedbackIgnoredEvent = RepoFeedbackEventPayload & {
  reason: "ipc_disabled" | "unmanaged_pr"
}

export type RepoFeedbackFinishedEvent = RepoFeedbackEventPayload & {
  exitCode: number
}

export type RepoSubscriptionDegradedEvent = {
  reason: "unauthenticated"
  errorMessage: string
}

/** Daemon-owned events produced outside feature plugin setup. */
export const daemonRuntimeEvents = {
  "repo.feedback.ignored": event<RepoFeedbackIgnoredEvent>(),
  "repo.feedback.finished": event<RepoFeedbackFinishedEvent>(),
  "repo.subscription.degraded": event<RepoSubscriptionDegradedEvent>(),
}
