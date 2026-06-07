import type { InboxItem } from "@goddard-ai/inbox/schema"

import { getInboxEntityKind } from "./entity-kind.ts"

const inboxPriorityRank = {
  normal: 0,
  low: 1,
} satisfies Record<InboxItem["priority"], number>

/** Returns whether one inbox row represents unread session attention. */
export function isUnreadSessionInboxItem(item: InboxItem) {
  return item.status === "unread" && getInboxEntityKind(item.entityId) === "session"
}

/** Returns whether any inbox row represents unread session attention. */
export function hasUnreadSessionInboxItems(items: readonly InboxItem[]) {
  return items.some(isUnreadSessionInboxItem)
}

/** Returns the next unread attention item by priority, then current unread freshness. */
export function getNextUnreadInboxAttentionItem(items: readonly InboxItem[]) {
  return (
    [...items]
      .filter((item) => item.status === "unread")
      .sort(
        (left, right) =>
          inboxPriorityRank[left.priority] - inboxPriorityRank[right.priority] ||
          right.updatedAt - left.updatedAt ||
          right.id.localeCompare(left.id),
      )[0] ?? null
  )
}
