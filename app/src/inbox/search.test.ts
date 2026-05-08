import type { InboxItem } from "@goddard-ai/schema/daemon"
import { expect, test } from "bun:test"

import {
  filterPreparedInboxItemsBySearch,
  getInboxSearchActiveFilterIds,
  parseInboxSearchQuery,
  prepareInboxSearchItems,
  replaceInboxSearchStatusFilters,
} from "./search.ts"

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
    status: "saved",
    scope: "Review sync",
    headline: "Pull request opened",
  } satisfies InboxItem

  const items = prepareInboxSearchItems([baseItem, pullRequestItem])

  expect(filterPreparedInboxItemsBySearch(items, "pull request")).toEqual([pullRequestItem])
  expect(filterPreparedInboxItemsBySearch(items, "rvw syc")).toEqual([pullRequestItem])
  expect(
    new Set(filterPreparedInboxItemsBySearch(items, "normal priority").map((item) => item.id)),
  ).toEqual(new Set([baseItem.id, pullRequestItem.id]))
})

test("inbox search supports status filters without fuzzy matching filter tokens", () => {
  const savedPullRequestItem = {
    ...baseItem,
    id: "inb_2",
    entityId: "pr_1",
    reason: "pull_request.created",
    status: "saved",
    scope: "Review sync",
    headline: "Pull request opened",
  } satisfies InboxItem
  const completedItem = {
    ...baseItem,
    id: "inb_3",
    status: "completed",
    scope: "Review sync",
  } satisfies InboxItem
  const items = prepareInboxSearchItems([baseItem, savedPullRequestItem, completedItem])
  const parsedSearch = parseInboxSearchQuery("is:saved rvw syc")

  expect(parsedSearch).toEqual({
    fuzzyQuery: "rvw syc",
    isActive: true,
    statuses: ["saved"],
  })
  expect(filterPreparedInboxItemsBySearch(items, parsedSearch)).toEqual([savedPullRequestItem])
  expect(filterPreparedInboxItemsBySearch(items, "status:completed")).toEqual([completedItem])
})

test("inbox search filter replacement swaps inline status filters", () => {
  expect(replaceInboxSearchStatusFilters("rvw syc status:archived", ["saved"])).toBe(
    "is:saved rvw syc",
  )
  expect(replaceInboxSearchStatusFilters("is:unread status:read blocked", ["unread", "read"])).toBe(
    "is:unread is:read blocked",
  )
})

test("inbox search reports active filters represented by status filters", () => {
  expect(getInboxSearchActiveFilterIds("review sync")).toEqual([])
  expect(getInboxSearchActiveFilterIds("is:saved review sync")).toEqual(["saved"])
  expect(getInboxSearchActiveFilterIds("status:read blocked")).toEqual(["unread"])
  expect(getInboxSearchActiveFilterIds("is:unread status:read blocked")).toEqual(["unread"])
  expect(getInboxSearchActiveFilterIds("is:saved status:archived")).toEqual(["saved", "archived"])
})
