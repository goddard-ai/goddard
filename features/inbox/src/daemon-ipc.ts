import { $type, defineIpcRoutes, http, ndjson } from "@goddard-ai/ipc"

import {
  BulkUpdateInboxItemsRequest,
  ListInboxRequest,
  UpdateInboxItemRequest,
  type BulkUpdateInboxItemsResponse,
  type InboxItemEvent,
  type ListInboxResponse,
  type UpdateInboxItemResponse,
} from "./schema.ts"

export const inboxIpcRoutes = defineIpcRoutes({
  inbox: http.resource("inbox", {
    /** Lists daemon-local inbox rows using daemon ordering and filtering. */
    list: http.post("list", {
      body: ListInboxRequest,
      response: $type<ListInboxResponse>(),
    }),
    /** Updates one daemon-local inbox row by entity id. */
    update: http.post("update", {
      body: UpdateInboxItemRequest,
      response: $type<UpdateInboxItemResponse>(),
    }),
    /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
    bulkUpdate: http.post("bulk-update", {
      body: BulkUpdateInboxItemsRequest,
      response: $type<BulkUpdateInboxItemsResponse>(),
    }),
    /** Emits daemon-published inbox item updates. */
    item: http.get("item-events", {
      response: ndjson.$type<InboxItemEvent>(),
    }),
  }),
})
