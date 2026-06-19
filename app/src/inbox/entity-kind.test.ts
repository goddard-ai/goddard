import { createFixtureInboxItem } from "@goddard-ai/fixtures"
import { expect, test } from "vitest"

import { getInboxEntityKind, isInboxEntityKind } from "./entity-kind.ts"

const baseItem = createFixtureInboxItem({
  id: "inb_1",
  entityId: "ses_1",
  updatedAt: 1_700_000_000_000,
  scope: "Inbox UI",
  headline: "Contract helpers are ready",
})

test("entity helpers expose daemon inbox entity families", () => {
  expect(getInboxEntityKind("ses_123")).toBe("session")
  expect(getInboxEntityKind("pr_123")).toBe("pullRequest")
  expect(isInboxEntityKind(baseItem, "session")).toBe(true)
  expect(isInboxEntityKind(baseItem, "pullRequest")).toBe(false)
})
