import { createFixtureInboxItem } from "@goddard-ai/fixtures"
import { expect, test } from "vitest"

import {
  getNextUnreadInboxAttentionItem,
  hasUnreadSessionInboxItems,
  isUnreadSessionInboxItem,
} from "./attention.ts"

test("isUnreadSessionInboxItem matches only unread session items", () => {
  expect(isUnreadSessionInboxItem(createFixtureInboxItem({ entityId: "ses_unread" }))).toBe(true)
  expect(
    isUnreadSessionInboxItem(createFixtureInboxItem({ entityId: "ses_read", status: "read" })),
  ).toBe(false)
  expect(isUnreadSessionInboxItem(createFixtureInboxItem({ entityId: "pr_unread" }))).toBe(false)
})

test("hasUnreadSessionInboxItems detects unread session attention", () => {
  expect(
    hasUnreadSessionInboxItems([
      createFixtureInboxItem({ entityId: "pr_unread" }),
      createFixtureInboxItem({ entityId: "ses_read", status: "read" }),
    ]),
  ).toBe(false)
  expect(
    hasUnreadSessionInboxItems([
      createFixtureInboxItem({ entityId: "pr_unread" }),
      createFixtureInboxItem({ entityId: "ses_unread" }),
    ]),
  ).toBe(true)
})

test("getNextUnreadInboxAttentionItem returns null with no unread items", () => {
  expect(
    getNextUnreadInboxAttentionItem([
      createFixtureInboxItem({ entityId: "ses_read", status: "read" }),
      createFixtureInboxItem({
        entityId: "ses_archived",
        status: "archived",
      }),
    ]),
  ).toBeNull()
})

test("getNextUnreadInboxAttentionItem prefers normal priority over low priority", () => {
  const lowPriorityFresh = createFixtureInboxItem({
    entityId: "ses_low",
    priority: "low",
    updatedAt: 20,
  })
  const normalPriorityStale = createFixtureInboxItem({
    entityId: "ses_normal",
    priority: "normal",
    updatedAt: 10,
  })

  expect(getNextUnreadInboxAttentionItem([lowPriorityFresh, normalPriorityStale])).toBe(
    normalPriorityStale,
  )
})

test("getNextUnreadInboxAttentionItem uses updatedAt freshness within equal priority", () => {
  const stale = createFixtureInboxItem({
    entityId: "ses_stale",
    updatedAt: 10,
  })
  const fresh = createFixtureInboxItem({
    entityId: "pr_fresh",
    updatedAt: 20,
  })

  expect(getNextUnreadInboxAttentionItem([stale, fresh])).toBe(fresh)
})
