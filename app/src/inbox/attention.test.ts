import { createFixtureInboxItem } from "@goddard-ai/fixtures"
import { expect, test } from "bun:test"

import {
  getNextUnreadInboxAttentionItem,
  hasUnreadSessionInboxItems,
  isUnreadSessionInboxItem,
} from "./attention.ts"

const inboxItemDefaults = { updatedAt: 1, scope: null, headline: null }

test("isUnreadSessionInboxItem matches only unread session items", () => {
  expect(
    isUnreadSessionInboxItem(
      createFixtureInboxItem({ ...inboxItemDefaults, entityId: "ses_unread" }),
    ),
  ).toBe(true)
  expect(
    isUnreadSessionInboxItem(
      createFixtureInboxItem({ ...inboxItemDefaults, entityId: "ses_read", status: "read" }),
    ),
  ).toBe(false)
  expect(
    isUnreadSessionInboxItem(
      createFixtureInboxItem({ ...inboxItemDefaults, entityId: "pr_unread" }),
    ),
  ).toBe(false)
})

test("hasUnreadSessionInboxItems detects unread session attention", () => {
  expect(
    hasUnreadSessionInboxItems([
      createFixtureInboxItem({ ...inboxItemDefaults, entityId: "pr_unread" }),
      createFixtureInboxItem({ ...inboxItemDefaults, entityId: "ses_read", status: "read" }),
    ]),
  ).toBe(false)
  expect(
    hasUnreadSessionInboxItems([
      createFixtureInboxItem({ ...inboxItemDefaults, entityId: "pr_unread" }),
      createFixtureInboxItem({ ...inboxItemDefaults, entityId: "ses_unread" }),
    ]),
  ).toBe(true)
})

test("getNextUnreadInboxAttentionItem returns null with no unread items", () => {
  expect(
    getNextUnreadInboxAttentionItem([
      createFixtureInboxItem({ ...inboxItemDefaults, entityId: "ses_read", status: "read" }),
      createFixtureInboxItem({
        ...inboxItemDefaults,
        entityId: "ses_archived",
        status: "archived",
      }),
    ]),
  ).toBeNull()
})

test("getNextUnreadInboxAttentionItem prefers normal priority over low priority", () => {
  const lowPriorityFresh = createFixtureInboxItem({
    ...inboxItemDefaults,
    entityId: "ses_low",
    priority: "low",
    updatedAt: 20,
  })
  const normalPriorityStale = createFixtureInboxItem({
    ...inboxItemDefaults,
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
    ...inboxItemDefaults,
    entityId: "ses_stale",
    updatedAt: 10,
  })
  const fresh = createFixtureInboxItem({
    ...inboxItemDefaults,
    entityId: "pr_fresh",
    updatedAt: 20,
  })

  expect(getNextUnreadInboxAttentionItem([stale, fresh])).toBe(fresh)
})
