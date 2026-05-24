import { $type, defineIpcRoutes, defineIpcSchema, http, ndjson } from "@goddard-ai/ipc"

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

export const inboxIpcRoutes = defineIpcRoutes({
  inbox: http.resource("inbox", {
    list: http.post("list", {
      body: ListInboxRequest,
      response: $type<ListInboxResponse>(),
    }),
    update: http.post("update", {
      body: UpdateInboxItemRequest,
      response: $type<UpdateInboxItemResponse>(),
    }),
    bulkUpdate: http.post("bulk-update", {
      body: BulkUpdateInboxItemsRequest,
      response: $type<BulkUpdateInboxItemsResponse>(),
    }),
    item: http.get("item-events", {
      response: ndjson.$type<InboxItemEvent>(),
    }),
  }),
})
