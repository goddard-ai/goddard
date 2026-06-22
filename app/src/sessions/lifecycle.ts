import { goddardSdk } from "~/sdk.ts"
import { invalidateSessionLifecycleEvent } from "./cache.ts"

/** Starts the app-wide daemon session lifecycle subscription for the current webview. */
export function startSessionLifecycleSubscription() {
  const controller = new AbortController()

  void (async () => {
    try {
      const events = await goddardSdk.events.stream(
        { names: ["session.lifecycle.updated", "session.lifecycle.deleted"] },
        { signal: controller.signal },
      )
      for await (const event of events) {
        invalidateSessionLifecycleEvent(event.payload)
      }
    } catch (error) {
      if (!controller.signal.aborted) {
        console.error("Failed to subscribe to session lifecycle updates.", error)
      }
    }
  })()

  return () => {
    controller.abort()
  }
}
