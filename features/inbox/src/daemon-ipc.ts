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
      description: "Daemon-local inbox row management.",
    }),
    /** Lists daemon-local inbox rows using daemon ordering and filtering. */
    list: http.post("list", {
      ...ipcMetadata({
        description: "Lists daemon-local inbox rows using daemon ordering and filtering.",
      }),
      body: ListInboxRequest,
      response: $type<ListInboxResponse>(),
    }),
    /** Updates one daemon-local inbox row by entity id. */
    update: http.post("update", {
      ...ipcMetadata({
        description: "Updates one daemon-local inbox row by entity id.",
      }),
      body: UpdateInboxItemRequest,
      response: $type<UpdateInboxItemResponse>(),
    }),
    /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
    bulkUpdate: http.post("bulk-update", {
      ...ipcMetadata({
        description: "Updates many daemon-local inbox rows with one shared daemon timestamp.",
      }),
      body: BulkUpdateInboxItemsRequest,
      response: $type<BulkUpdateInboxItemsResponse>(),
    }),
    /** Validates and completes the inbox row for one daemon-managed session. */
    completeSession: http.post("complete-session", {
      ...ipcMetadata({
        description: "Validates and completes the inbox row for one daemon-managed session.",
      }),
      body: CompleteSessionInboxItemRequest,
      response: $type<CompleteSessionInboxItemResponse>(),
    }),
    /** Streams daemon-published inbox item updates. */
    streamItems: http.get("stream-items", {
      ...ipcMetadata({
        description: "Streams daemon-published inbox item updates.",
      }),
      response: ndjson.$type<InboxItem>(),
    }),
  }),
})
