import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { inboxIpcRoutes } from "./daemon-ipc.ts"
import type {
  BulkUpdateInboxItemsRequest,
  InboxItemEvent,
  ListInboxRequest,
  UpdateInboxItemRequest,
} from "./schema.ts"

export const inboxSdkPlugin = defineSdkPlugin({
  name: "inbox",
  ipcRoutes: inboxIpcRoutes,
  wrap({ client }) {
    return {
      inbox: {
        /** Lists daemon-local inbox rows using daemon ordering and filtering. */
        list: (input: ListInboxRequest = {}) => client.inbox.list(input),

        /** Updates one daemon-local inbox row by entity id. */
        update: (input: UpdateInboxItemRequest) => client.inbox.update(input),

        /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
        bulkUpdate: (input: BulkUpdateInboxItemsRequest) => client.inbox.bulkUpdate(input),

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
