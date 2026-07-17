import { startDaemonEventStream } from "~/lib/daemon-event-stream.ts"
import { goddardSdk } from "~/sdk.ts"
import { invalidateAllSessionViews, invalidateSessionLifecycleEvent } from "./cache.ts"

/** Starts the app-wide daemon session lifecycle subscription for the current webview. */
export function startSessionLifecycleSubscription() {
  return startDaemonEventStream({
    failureLogMessage: "app.session.lifecycle_subscription_failed",
    streamName: "session.lifecycle",
    open: (signal) =>
      goddardSdk.events.stream(
        { names: ["session.lifecycle.updated", "session.lifecycle.deleted"] },
        { signal },
      ),
    reconcile: invalidateAllSessionViews,
    onEvent: (event) => invalidateSessionLifecycleEvent(event.payload),
  })
}
