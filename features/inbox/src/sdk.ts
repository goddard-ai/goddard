import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import type {
  BulkUpdateInboxItemsRequest,
  InboxItemEvent,
  ListInboxRequest,
  UpdateInboxItemRequest,
} from "./schema.ts"

export const inboxSdkPlugin = defineSdkPlugin({
  name: "inbox",
  create({ client }: { client: DaemonIpcClient }) {
    return {
      inbox: {
        /** Lists daemon-local inbox rows using daemon ordering and filtering. */
        list: async (input: ListInboxRequest = {}) => client.send("inbox.list", input),

        /** Updates one daemon-local inbox row by entity id. */
        update: async (input: UpdateInboxItemRequest) => client.send("inbox.update", input),

        /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
        bulkUpdate: async (input: BulkUpdateInboxItemsRequest) =>
          client.send("inbox.bulkUpdate", input),

        /** Subscribes to daemon-published inbox item updates. */
        subscribe: (
          onMessage: (event: InboxItemEvent) => void,
          onError?: (error: unknown) => void,
        ) => client.subscribe("inbox.item", onMessage, onError),
      },
    }
  },
})
