import type { InboxStatus, ListInboxRequest } from "@goddard-ai/inbox/schema"

import { DEFAULT_INBOX_FILTER_ID, inboxFilterDefinitions, type InboxFilterId } from "./filters.ts"

export const INBOX_LIST_LIMIT = 50
export const INBOX_ATTENTION_LIST_LIMIT = 100

/** Inputs supported by the app inbox list query resolver. */
export type InboxListRequestInput = {
  filterId?: InboxFilterId
  statuses?: readonly InboxStatus[]
}

/** Returns the daemon list request for one app-supported inbox filter. */
export function getInboxListRequest(input: InboxListRequestInput = {}) {
  const filterId = input.filterId ?? DEFAULT_INBOX_FILTER_ID
  const filter = inboxFilterDefinitions[filterId]

  return {
    statuses: [...(input.statuses ?? filter.statuses)],
    limit: INBOX_LIST_LIMIT,
  } satisfies ListInboxRequest
}

/** Returns the app-shell request for unread items that can claim attention. */
export function getUnreadInboxAttentionListRequest() {
  return {
    statuses: ["unread"],
    limit: INBOX_ATTENTION_LIST_LIMIT,
  } satisfies ListInboxRequest
}
