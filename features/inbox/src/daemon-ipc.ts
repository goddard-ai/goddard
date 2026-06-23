import { $type, defineIpcRoutes, http, ipcMetadata, ndjson } from "@goddard-ai/ipc"

import {
  BulkUpdateInboxItemsRequest,
  CompleteSessionInboxItemRequest,
  ListInboxRequest,
  UpdateInboxItemRequest,
  type BulkUpdateInboxItemsResponse,
  type CompleteSessionInboxItemResponse,
  type ListInboxResponse,
  type UpdateInboxItemResponse,
} from "./schema.ts"

export const inboxIpcRoutes = defineIpcRoutes({
  inbox: http.resource("inbox", {
    ...ipcMetadata({
      description: "Inbox row management.",
    }),
    /** Lists inbox rows using stable ordering and filtering. */
    list: http.post("list", {
      ...ipcMetadata({
        description: "Lists inbox rows using stable ordering and filtering.",
      }),
      body: ListInboxRequest,
      response: $type<ListInboxResponse>(),
    }),
    /** Updates one inbox row by entity id. */
    update: http.post("update", {
      ...ipcMetadata({
        description: "Updates one inbox row by entity id.",
      }),
      body: UpdateInboxItemRequest,
      response: $type<UpdateInboxItemResponse>(),
    }),
    /** Updates many inbox rows with one shared timestamp. */
    bulkUpdate: http.post("bulk-update", {
      ...ipcMetadata({
        description: "Updates many inbox rows with one shared timestamp.",
      }),
      body: BulkUpdateInboxItemsRequest,
      response: $type<BulkUpdateInboxItemsResponse>(),
    }),
    /** Validates and completes the inbox row for one session. */
    completeSession: http.post("complete-session", {
      ...ipcMetadata({
        description: "Validates and completes the inbox row for one session.",
      }),
      body: CompleteSessionInboxItemRequest,
      response: $type<CompleteSessionInboxItemResponse>(),
    }),
    /** Streams inbox item updates. */
    streamItems: http.get("stream-items", {
      ...ipcMetadata({
        description: "Streams inbox item updates.",
      }),
      response: ndjson.$type<InboxItem>(),
    }),
  }),
})
