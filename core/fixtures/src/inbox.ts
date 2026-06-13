import type { InboxItem, ListInboxResponse } from "@goddard-ai/inbox/schema"

import { fixtureInboxItemId, fixtureSessionId } from "./ids.ts"
import { fixtureNow } from "./time.ts"

export function createFixtureInboxItem(overrides: Partial<InboxItem> = {}): InboxItem {
  const entityId = overrides.entityId ?? fixtureSessionId("session_1")

  return {
    id: overrides.id ?? fixtureInboxItemId(entityId),
    entityId,
    headline: "Review the latest agent update.",
    priority: "normal",
    readAt: null,
    reason: "session.turn_ended",
    scope: "Fixture session",
    status: "unread",
    turnId: null,
    updatedAt: fixtureNow,
    ...overrides,
  }
}

export function createListInboxResponse(
  items: InboxItem[] = [createFixtureInboxItem()],
  overrides: Partial<Omit<ListInboxResponse, "items">> = {},
): ListInboxResponse {
  return {
    hasMore: false,
    nextCursor: null,
    items,
    ...overrides,
  }
}
