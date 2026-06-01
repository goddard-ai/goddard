import type { InboxItem } from "@goddard-ai/inbox/schema"
import { expect, test } from "bun:test"

import { getInboxEntityKind, isInboxEntityKind } from "./entity-kind.ts"

const baseItem = {
  id: "inb_1",
  entityId: "ses_1",
  reason: "session.turn_ended",
  status: "unread",
  priority: "normal",
  updatedAt: 1_700_000_000_000,
  readAt: null,
  scope: "Inbox UI",
  headline: "Contract helpers are ready",
  turnId: null,
} satisfies InboxItem

test("entity helpers expose daemon inbox entity families", () => {
  expect(getInboxEntityKind("ses_123")).toBe("session")
  expect(getInboxEntityKind("pr_123")).toBe("pullRequest")
  expect(isInboxEntityKind(baseItem, "session")).toBe(true)
  expect(isInboxEntityKind(baseItem, "pullRequest")).toBe(false)
})
