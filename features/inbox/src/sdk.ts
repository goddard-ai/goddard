import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { inboxIpcRoutes } from "./daemon-ipc.ts"
import type { InboxItemEvent } from "./schema.ts"

export const inboxSdkPlugin = defineSdkPlugin({
  name: "inbox",
  ipcRoutes: inboxIpcRoutes,
  wrap({ client }) {
    return {
      inbox: {
        /** Subscribes to daemon-published inbox item updates. */
        subscribe: async (onMessage: (payload: InboxItemEvent) => void) => {
          const controller = new AbortController()
          const events = await client.inbox.item(undefined, { signal: controller.signal })

          void (async () => {
            try {
              for await (const event of events) {
                if (controller.signal.aborted) {
                  break
                }
                onMessage(event)
              }
            } catch (error) {
              if (!controller.signal.aborted) {
                throw error
              }
            }
          })()

          return () => {
            controller.abort()
          }
        },
      },
    }
  },
})
