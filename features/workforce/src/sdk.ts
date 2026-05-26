import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { workforceIpcRoutes } from "./daemon-ipc.ts"
import type { SubscribeWorkforceEventsRequest, WorkforceEventEnvelope } from "./schema.ts"

export const workforceSdkPlugin = defineSdkPlugin({
  name: "workforce",
  ipcRoutes: workforceIpcRoutes,
  wrap({ client }) {
    return {
      workforce: {
        /** Subscribes to live daemon-published workforce ledger events for one repository root. */
        subscribe: async (
          input: SubscribeWorkforceEventsRequest,
          onEvent: (event: WorkforceEventEnvelope["event"]) => void,
        ): Promise<() => void> => {
          const controller = new AbortController()
          const events = await client.workforce.event(input, { signal: controller.signal })
          void (async () => {
            for await (const payload of events) {
              if (controller.signal.aborted) {
                break
              }
              onEvent(payload.event)
            }
          })()
          return () => controller.abort()
        },
      },
    }
  },
})
