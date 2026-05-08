import { $type, defineIpcSchema } from "@goddard-ai/ipc"

import {
  BulkUpdateInboxItemsRequest,
  ListInboxRequest,
  UpdateInboxItemRequest,
  type BulkUpdateInboxItemsResponse,
  type InboxItemEvent,
  type ListInboxResponse,
  type UpdateInboxItemResponse,
} from "./schema.ts"

export const inboxIpcSchema = defineIpcSchema({
  requests: {
    "inbox.list": {
      payload: ListInboxRequest,
      response: $type<ListInboxResponse>(),
    },
    "inbox.update": {
      payload: UpdateInboxItemRequest,
      response: $type<UpdateInboxItemResponse>(),
    },
    "inbox.bulkUpdate": {
      payload: BulkUpdateInboxItemsRequest,
      response: $type<BulkUpdateInboxItemsResponse>(),
    },
  },
  streams: {
    "inbox.item": {
      payload: $type<InboxItemEvent>(),
    },
  },
})
