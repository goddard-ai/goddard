import { definePlugin } from "@goddard-ai/daemon-plugin"

import { inboxIpcSchema } from "./daemon-ipc.ts"
import type {
  BulkUpdateInboxItemsRequest,
  BulkUpdateInboxItemsResponse,
  ListInboxRequest,
  ListInboxResponse,
  UpdateInboxItemRequest,
  UpdateInboxItemResponse,
} from "./schema.ts"

type InboxHandlerContext = {
  inboxManager: {
    listInboxItems: (input: ListInboxRequest) => ListInboxResponse | Promise<ListInboxResponse>
    updateInboxItem: (
      input: UpdateInboxItemRequest,
    ) => UpdateInboxItemResponse | Promise<UpdateInboxItemResponse>
    bulkUpdateInboxItems: (
      input: BulkUpdateInboxItemsRequest,
    ) => BulkUpdateInboxItemsResponse | Promise<BulkUpdateInboxItemsResponse>
  }
}

export const inboxPlugin = definePlugin({
  name: "inbox",
  ipc: inboxIpcSchema,
  setup({ inboxManager }: InboxHandlerContext) {
    return {
      requestHandlers: {
        "inbox.list": async (payload) => inboxManager.listInboxItems(payload),
        "inbox.update": async (payload) => inboxManager.updateInboxItem(payload),
        "inbox.bulkUpdate": async (payload) => inboxManager.bulkUpdateInboxItems(payload),
      },
    }
  },
})
