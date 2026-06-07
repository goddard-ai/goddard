import type { InboxItem } from "@goddard-ai/inbox/schema"
import { expect, test } from "bun:test"

import {
  getNextUnreadInboxAttentionItem,
  hasUnreadSessionInboxItems,
  isUnreadSessionInboxItem,
} from "./attention.ts"

function createInboxItem(input: Partial<InboxItem> & Pick<InboxItem, "entityId">): InboxItem {
  return {
    id: input.id ?? `inb_${input.entityId}`,
    entityId: input.entityId,
    reason: input.reason ?? "session.turn_ended",
    status: input.status ?? "unread",
    priority: input.priority ?? "normal",
    updatedAt: input.updatedAt ?? 1,
    readAt: input.readAt ?? null,
    scope: input.scope ?? null,
    headline: input.headline ?? null,
    turnId: input.turnId ?? null,
  }
}

test("isUnreadSessionInboxItem matches only unread session items", () => {
  expect(isUnreadSessionInboxItem(createInboxItem({ entityId: "ses_unread" }))).toBe(true)
  expect(isUnreadSessionInboxItem(createInboxItem({ entityId: "ses_read", status: "read" }))).toBe(
    false,
  )
  expect(isUnreadSessionInboxItem(createInboxItem({ entityId: "pr_unread" }))).toBe(false)
})

test("hasUnreadSessionInboxItems detects unread session attention", () => {
  expect(
    hasUnreadSessionInboxItems([
      createInboxItem({ entityId: "pr_unread" }),
      createInboxItem({ entityId: "ses_read", status: "read" }),
    ]),
  ).toBe(false)
  expect(
    hasUnreadSessionInboxItems([
      createInboxItem({ entityId: "pr_unread" }),
      createInboxItem({ entityId: "ses_unread" }),
    ]),
  ).toBe(true)
})

test("getNextUnreadInboxAttentionItem returns null with no unread items", () => {
  expect(
    getNextUnreadInboxAttentionItem([
      createInboxItem({ entityId: "ses_read", status: "read" }),
      createInboxItem({ entityId: "ses_archived", status: "archived" }),
    ]),
  ).toBeNull()
})

test("getNextUnreadInboxAttentionItem prefers normal priority over low priority", () => {
  const lowPriorityFresh = createInboxItem({
    entityId: "ses_low",
    priority: "low",
    updatedAt: 20,
  })
  const normalPriorityStale = createInboxItem({
    entityId: "ses_normal",
    priority: "normal",
    updatedAt: 10,
  })

  expect(getNextUnreadInboxAttentionItem([lowPriorityFresh, normalPriorityStale])).toBe(
    normalPriorityStale,
  )
})

test("getNextUnreadInboxAttentionItem uses updatedAt freshness within equal priority", () => {
  const stale = createInboxItem({ entityId: "ses_stale", updatedAt: 10 })
  const fresh = createInboxItem({ entityId: "pr_fresh", updatedAt: 20 })

  expect(getNextUnreadInboxAttentionItem([stale, fresh])).toBe(fresh)
})
