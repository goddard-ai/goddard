import { writeRendererError } from "~/lib/renderer-log-capture.ts"
import { goddardSdk } from "~/sdk.ts"
import { invalidateInboxQueries } from "./mutations.ts"
import { handleInboxItemsLoaded } from "./session-visit.ts"

/** Starts the page-owned realtime inbox subscription. */
export function startInboxUpdateStream() {
  const controller = new AbortController()

  void (async () => {
    try {
      const events = await goddardSdk.events.stream(
        { names: ["inbox.item.updated"] },
        { signal: controller.signal },
      )
      for await (const event of events) {
        handleInboxItemsLoaded([event.payload])
        invalidateInboxQueries()
      }
    } catch (error) {
      if (!controller.signal.aborted) {
        writeRendererError("app.inbox.subscription_failed", error)
      }
    }
  })()

  return () => {
    controller.abort()
  }
}
