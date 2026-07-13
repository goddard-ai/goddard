import type { DaemonSession, SessionLifecycleEvent } from "@goddard-ai/sdk"

import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"

/** Invalidates every cached daemon session list variant. */
export function invalidateSessionLists() {
  queryClient.invalidate(goddardSdk.session.list)
}

/** Invalidates the detail and latest transcript reads for one session. */
export function invalidateSessionDetail(sessionId: DaemonSession["id"]) {
  queryClient.invalidate(goddardSdk.session.get, [{ id: sessionId }])
  queryClient.invalidate(goddardSdk.session.history, [{ id: sessionId }])
  queryClient.invalidate(goddardSdk.session.worktree.get, [{ id: sessionId }])
  queryClient.invalidate(goddardSdk.session.worktree.mergeReadiness, [{ id: sessionId }])
}

/** Invalidates every cached session view after the lifecycle stream reconnects. */
export function invalidateAllSessionViews() {
  invalidateSessionLists()
  queryClient.invalidate(goddardSdk.session.get)
  queryClient.invalidate(goddardSdk.session.history)
}

/** Invalidates session reads that can change after a session lifecycle mutation. */
export function invalidateSessionViews(sessionId: DaemonSession["id"]) {
  invalidateSessionLists()
  invalidateSessionDetail(sessionId)
}

/** Invalidates the current change summary for one session workspace. */
export function invalidateSessionChanges(sessionId: DaemonSession["id"]) {
  queryClient.invalidate(goddardSdk.session.changes, [{ id: sessionId }])
}

/** Invalidates launch-time data cached before a new session is created. */
export function invalidateSessionLaunchPreview() {
  queryClient.invalidate(goddardSdk.session.launchPreview)
}

/** Invalidates cached session reads affected by one daemon-published lifecycle update. */
export function invalidateSessionLifecycleEvent(event: SessionLifecycleEvent) {
  if (event.kind === "sessionDeleted") {
    invalidateSessionLists()
    return
  }

  invalidateSessionViews(event.session.id)
}
