import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"
import { event } from "@goddard-ai/sdk-plugin"

import type { PullRequestId } from "./schema.ts"

export type PullRequestAttentionEvent = {
  pullRequestId: PullRequestId
  scope: AttentionScope
  headline: AttentionHeadline
  turnId: string | null
}

export const pullRequestEvents = {
  "pull_request.created": event<PullRequestAttentionEvent>(),
  "pull_request.updated": event<PullRequestAttentionEvent>(),
}
