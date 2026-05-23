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
  extend({ client }) {
    return {
      inbox: {
        /** Lists daemon-local inbox rows using daemon ordering and filtering. */
        list: (input: ListInboxRequest = {}) => client.send("inbox.list", input),

        /** Updates one daemon-local inbox row by entity id. */
        update: (input: UpdateInboxItemRequest) => client.send("inbox.update", input),

        /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
        bulkUpdate: (input: BulkUpdateInboxItemsRequest) => client.send("inbox.bulkUpdate", input),

        /** Subscribes to daemon-published inbox item updates. */
        subscribe: (onMessage: (payload: InboxItemEvent) => void) =>
          client.subscribe("inbox.item", onMessage),
      },
    }
  },
})
