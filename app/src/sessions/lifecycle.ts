import type { SessionLifecycleEvent } from "@goddard-ai/sdk"

import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"

/** Refreshes cached session reads affected by one daemon-published lifecycle update. */
export function invalidateSessionLifecycleQueries(event: SessionLifecycleEvent) {
  if (event.kind === "sessionDeleted") {
    queryClient.invalidate(goddardSdk.session.list)
    return
  }

  queryClient.invalidate(goddardSdk.session.get, [{ id: event.session.id }])
  queryClient.invalidate(goddardSdk.session.history, [{ id: event.session.id }])
  queryClient.invalidate(goddardSdk.session.list)
}

/** Starts the app-wide daemon session lifecycle subscription for the current webview. */
export function startSessionLifecycleSubscription() {
  let active = true
  let unsubscribe: (() => void) | null = null

  void goddardSdk.session.lifecycle
    .subscribe((event) => {
      if (active) {
        invalidateSessionLifecycleQueries(event)
      }
    })
    .then(
      (nextUnsubscribe) => {
        if (active) {
          unsubscribe = nextUnsubscribe
        } else {
          nextUnsubscribe()
        }
      },
      (error) => {
        if (active) {
          console.error("Failed to subscribe to session lifecycle updates.", error)
        }
      },
    )

  return () => {
    active = false
    unsubscribe?.()
    unsubscribe = null
  }
}
