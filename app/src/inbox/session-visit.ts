import type { InboxEntityId, InboxItem } from "@goddard-ai/inbox/schema"

import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { applyInboxItemsToCache } from "./cache.ts"
import { getInboxEntityKind } from "./entity-kind.ts"

const knownInboxItemsByEntityId = new Map<InboxEntityId, InboxItem>()
const pendingReadEntityIds = new Set<InboxEntityId>()
const visitedSessionEntityIds = new Set<InboxEntityId>()

function isSessionEntityId(entityId: InboxEntityId) {
  return getInboxEntityKind(entityId) === "session"
}

function isUnreadVisitedSessionItem(item: InboxItem) {
  return (
    item.status === "unread" &&
    isSessionEntityId(item.entityId) &&
    visitedSessionEntityIds.has(item.entityId)
  )
}

async function markVisitedInboxItemRead(item: InboxItem) {
  if (!isUnreadVisitedSessionItem(item) || pendingReadEntityIds.has(item.entityId)) {
    return
  }

  pendingReadEntityIds.add(item.entityId)
  try {
    const result = await goddardSdk.inbox.update({
      entityId: item.entityId,
      status: "read",
    })
    knownInboxItemsByEntityId.set(result.item.entityId, result.item)
    applyInboxItemsToCache([result.item])
    queryClient.invalidate(goddardSdk.inbox.list)
  } catch (error) {
    console.error("Failed to mark visited inbox session read.", error)
  } finally {
    pendingReadEntityIds.delete(item.entityId)
  }
}

/** Records successfully loaded inbox items and applies read-on-visit behavior when needed. */
export function handleInboxItemsLoaded(items: readonly InboxItem[]) {
  for (const item of items) {
    knownInboxItemsByEntityId.set(item.entityId, item)

    if (isUnreadVisitedSessionItem(item)) {
      void markVisitedInboxItemRead(item)
    }
  }
}

/** Marks an unread session inbox row read after its associated session has loaded successfully. */
export function markInboxSessionVisited(sessionId: InboxEntityId) {
  if (!isSessionEntityId(sessionId)) {
    return
  }

  visitedSessionEntityIds.add(sessionId)

  const item = knownInboxItemsByEntityId.get(sessionId)
  if (item) {
    void markVisitedInboxItemRead(item)
  }
}

export function resetInboxSessionVisitStateForTest() {
  knownInboxItemsByEntityId.clear()
  pendingReadEntityIds.clear()
  visitedSessionEntityIds.clear()
}
