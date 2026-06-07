import type { InboxItem } from "@goddard-ai/inbox/schema"

import { getInboxEntityKind } from "./entity-kind.ts"

/** Returns whether one inbox row represents unread session attention. */
export function isUnreadSessionInboxItem(item: InboxItem) {
  return item.status === "unread" && getInboxEntityKind(item.entityId) === "session"
}

/** Returns whether any inbox row represents unread session attention. */
export function hasUnreadSessionInboxItems(items: readonly InboxItem[]) {
  return items.some(isUnreadSessionInboxItem)
}
