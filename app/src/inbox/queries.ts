import type { ListInboxRequest } from "@goddard-ai/schema/daemon"

import {
  DEFAULT_INBOX_FILTER_ID,
  inboxFilterDefinitions,
  type InboxFilterId,
} from "./presentation.ts"

export const INBOX_LIST_LIMIT = 50

/** Returns the daemon list request for one app-supported inbox filter. */
export function getInboxListRequest(filterId: InboxFilterId = DEFAULT_INBOX_FILTER_ID) {
  const filter = inboxFilterDefinitions[filterId]

  return {
    statuses: [...filter.statuses],
    limit: INBOX_LIST_LIMIT,
  } satisfies ListInboxRequest
}
