import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"
import { event } from "@goddard-ai/sdk-plugin"

import type { PullRequestId } from "./schema.ts"

export type PullRequestAttentionEvent = {
  pullRequestId: PullRequestId
  scope: AttentionScope
  headline: AttentionHeadline
  turnId: string | null
}

export type PullRequestFeedbackEventPayload = {
  repository: string
  owner: string
  repo: string
  prNumber: number
  feedbackType: "comment" | "review"
}

export type PullRequestFeedbackIgnoredEvent = PullRequestFeedbackEventPayload & {
  reason: "unmanaged_pr"
}

export type PullRequestFeedbackFinishedEvent = PullRequestFeedbackEventPayload & {
  exitCode: number
}

export const pullRequestEvents = {
  "pull_request.created": event<PullRequestAttentionEvent>(),
  "pull_request.updated": event<PullRequestAttentionEvent>(),
  "pull_request.feedback.ignored": event<PullRequestFeedbackIgnoredEvent>(),
  "pull_request.feedback.finished": event<PullRequestFeedbackFinishedEvent>(),
}
