import { createFixtureInboxItem } from "@goddard-ai/fixtures"
import { expect, test } from "vitest"

import { getInboxItemPrimaryText, getInboxItemSecondaryText } from "./text.ts"

const baseItem = createFixtureInboxItem({
  id: "inb_1",
  entityId: "ses_1",
  updatedAt: 1_700_000_000_000,
  scope: "Inbox UI",
  headline: "Contract helpers are ready",
})

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
