import { defineDaemonPlugin } from "@goddard-ai/daemon-plugin"

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

/** Creates daemon request handlers for feature-owned inbox IPC routes. */
export function createInboxRequestHandlers({ inboxManager }: InboxHandlerContext) {
  return {
    "inbox.list": async (payload: ListInboxRequest) => inboxManager.listInboxItems(payload),
    "inbox.update": async (payload: UpdateInboxItemRequest) =>
      inboxManager.updateInboxItem(payload),
    "inbox.bulkUpdate": async (payload: BulkUpdateInboxItemsRequest) =>
      inboxManager.bulkUpdateInboxItems(payload),
  }
}

export const inboxDaemonPlugin = defineDaemonPlugin({
  name: "inbox",
  ipc: inboxIpcSchema,
  createRequestHandlers: createInboxRequestHandlers,
})
