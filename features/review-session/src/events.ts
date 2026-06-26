import { event } from "@goddard-ai/sdk-plugin"
import type { SessionId } from "@goddard-ai/session/schema"

import type { ReviewSessionState } from "./schema.ts"

export type ReviewSessionMountedEvent = {
  sessionId: SessionId
  reviewSessionId: string
  agentBranch: string
  reviewBranch: string
}

export type ReviewSessionUnmountedEvent = {
  sessionId: SessionId
  reason: string
  reviewSessionId: string
}

export type ReviewSessionReplacedEvent = {
  sessionId: SessionId
  replacedBySessionId?: SessionId
  previousSessionId?: SessionId | null
  previousReviewSessionId?: string
}

export type ReviewSessionSyncCompletedEvent = {
  sessionId: SessionId
  reason: string
  warningCount: number
  lastSync: ReviewSessionState["lastSync"]
}

export const reviewSessionEvents = {
  "review_session.mounted": event<ReviewSessionMountedEvent>({ debug: "review_session" }),
  "review_session.unmounted": event<ReviewSessionUnmountedEvent>({ debug: "review_session" }),
  "review_session.replaced": event<ReviewSessionReplacedEvent>({ debug: "review_session" }),
  "review_session.sync.completed": event<ReviewSessionSyncCompletedEvent>({
    debug: "review_session",
  }),
}
