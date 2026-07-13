import { writeRendererError } from "~/lib/renderer-log-capture.ts"
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
        writeRendererError("app.session.lifecycle_subscription_failed", error)
      }
    }
  })()

  return () => {
    controller.abort()
  }
}
