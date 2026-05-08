import type { InboxItem } from "@goddard-ai/schema/daemon"
import { expect, test } from "bun:test"

import { getInboxItemPrimaryText, getInboxItemSecondaryText } from "./text.ts"

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

test("inbox row text prefers scope and headline with daemon reason fallback", () => {
  expect(getInboxItemPrimaryText(baseItem)).toBe("Inbox UI")
  expect(getInboxItemSecondaryText(baseItem)).toBe("Contract helpers are ready")

  expect(
    getInboxItemPrimaryText({
      ...baseItem,
      scope: null,
    }),
  ).toBe("Session")
  expect(
    getInboxItemSecondaryText({
      ...baseItem,
      headline: null,
    }),
  ).toBe("Session turn ended")
})
