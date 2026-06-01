import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"
import mitt from "mitt"

import type { PullRequestId } from "../schema.ts"

type PullRequestAttentionEvent = {
  pullRequestId: PullRequestId
  scope: AttentionScope
  headline: AttentionHeadline
  turnId: string | null
}

export const pullRequestEvents = mitt<{
  "lifecycle.created": PullRequestAttentionEvent
  "lifecycle.updated": PullRequestAttentionEvent
}>()

export type PullRequestEventEmitter = typeof pullRequestEvents
