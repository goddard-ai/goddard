import type { InboxItem } from "@goddard-ai/schema/daemon"
import { expect, test } from "bun:test"

import { filterInboxItemsBySearch } from "./search.ts"

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

test("inbox search fuzzy matches human-visible row text", () => {
  const pullRequestItem = {
    ...baseItem,
    id: "inb_2",
    entityId: "pr_1",
    reason: "pull_request.created",
    scope: "Review sync",
    headline: "Pull request opened",
  } satisfies InboxItem

  expect(filterInboxItemsBySearch([baseItem, pullRequestItem], "pull request")).toEqual([
    pullRequestItem,
  ])
  expect(filterInboxItemsBySearch([baseItem, pullRequestItem], "rvw syc")).toEqual([
    pullRequestItem,
  ])
  expect(
    new Set(
      filterInboxItemsBySearch([baseItem, pullRequestItem], "normal priority").map(
        (item) => item.id,
      ),
    ),
  ).toEqual(new Set([baseItem.id, pullRequestItem.id]))
})
