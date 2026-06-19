import { expect, test } from "vitest"

import { allInboxStatuses, DEFAULT_INBOX_FILTER_ID, inboxFilterDefinitions } from "./filters.ts"
import { getInboxListRequest } from "./queries.ts"

test("default inbox list request asks the daemon for unread and read active items", () => {
  expect(getInboxListRequest({ filterId: DEFAULT_INBOX_FILTER_ID })).toEqual({
    statuses: ["unread", "read"],
    limit: 50,
  })
})

test("search inbox list request can ask the daemon for every status", () => {
  expect(getInboxListRequest({ statuses: allInboxStatuses })).toEqual({
    statuses: ["unread", "read", "saved", "replied", "completed", "archived"],
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
