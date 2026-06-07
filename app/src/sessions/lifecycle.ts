import { goddardSdk } from "~/sdk.ts"
import { invalidateSessionLifecycleEvent } from "./cache.ts"

/** Starts the app-wide daemon session lifecycle subscription for the current webview. */
export function startSessionLifecycleSubscription() {
  let active = true
  let unsubscribe: (() => void) | null = null

  void goddardSdk.session.lifecycle
    .subscribe((event) => {
      if (active) {
        invalidateSessionLifecycleEvent(event)
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
