import { event } from "@goddard-ai/daemon-plugin"
import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"

import type { PullRequestId } from "../schema.ts"

type PullRequestAttentionEvent = {
  pullRequestId: PullRequestId
  scope: AttentionScope
  headline: AttentionHeadline
  turnId: string | null
}

export const pullRequestEvents = {
  "pull_request.created": event<PullRequestAttentionEvent>(),
  "pull_request.updated": event<PullRequestAttentionEvent>(),
}
