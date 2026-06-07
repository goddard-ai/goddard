import type { InboxItem } from "@goddard-ai/inbox/schema"
import { expect, test } from "bun:test"

import { hasUnreadSessionInboxItems, isUnreadSessionInboxItem } from "./attention.ts"

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
