import type { InboxItem } from "@goddard-ai/schema/daemon"
import { expect, test } from "bun:test"

import {
  DEFAULT_INBOX_FILTER_ID,
  filterInboxItemsBySearch,
  formatInboxUpdatedTime,
  getInboxEntityKind,
  getInboxItemPrimaryText,
  getInboxItemSecondaryText,
  getInboxPriorityLabel,
  getInboxReasonLabel,
  getInboxStatusLabel,
  inboxFilterDefinitions,
} from "./presentation.ts"
import { getInboxListRequest } from "./queries.ts"

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

test("default inbox list request asks the daemon for unread and read active items", () => {
  expect(getInboxListRequest(DEFAULT_INBOX_FILTER_ID)).toEqual({
    statuses: ["unread", "read"],
    limit: 50,
  })
})

test("inbox filters split saved, replied, completed, and archived states", () => {
  expect(inboxFilterDefinitions.unread.statuses).toEqual(["unread", "read"])
  expect(inboxFilterDefinitions.saved.statuses).toEqual(["saved"])
  expect(inboxFilterDefinitions.replied.statuses).toEqual(["replied"])
  expect(inboxFilterDefinitions.completed.statuses).toEqual(["completed"])
  expect(inboxFilterDefinitions.archived.statuses).toEqual(["archived"])
})

test("presentation labels expose daemon inbox terms without changing state", () => {
  expect(getInboxEntityKind("ses_123")).toBe("session")
  expect(getInboxEntityKind("pr_123")).toBe("pullRequest")
  expect(getInboxStatusLabel("replied")).toBe("Replied")
  expect(getInboxPriorityLabel("low")).toBe("Low priority")
  expect(getInboxReasonLabel("pull_request.updated")).toBe("Pull request updated")
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

test("inbox search matches human-visible row text and preserves daemon order", () => {
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
  expect(filterInboxItemsBySearch([baseItem, pullRequestItem], "normal priority")).toEqual([
    baseItem,
    pullRequestItem,
  ])
})

test("compact updated time labels use minutes, hours, days, and dates", () => {
  const now = Date.UTC(2026, 0, 10, 12, 0)

  expect(formatInboxUpdatedTime(now, now)).toBe("now")
  expect(formatInboxUpdatedTime(now - 5 * 60_000, now)).toBe("5m")
  expect(formatInboxUpdatedTime(now - 2 * 60 * 60_000, now)).toBe("2h")
  expect(formatInboxUpdatedTime(now - 3 * 24 * 60 * 60_000, now)).toBe("3d")
  expect(formatInboxUpdatedTime(now - 8 * 24 * 60 * 60_000, now)).toBe("Jan 2")
})
