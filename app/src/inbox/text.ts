import type { InboxItem } from "@goddard-ai/inbox/schema"

import { getInboxEntityLabel } from "./entity-kind.ts"
import { getInboxReasonLabel } from "./labels.ts"

/** Returns the primary row text, preferring the daemon-supplied stable scope. */
export function getInboxItemPrimaryText(item: InboxItem) {
  return item.scope?.trim() || getInboxEntityLabel(item.entityId)
}

/** Returns the secondary row text, preferring the daemon-supplied turn headline. */
export function getInboxItemSecondaryText(item: InboxItem) {
  return item.headline?.trim() || getInboxReasonLabel(item.reason)
}
