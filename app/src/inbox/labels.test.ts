import { expect, test } from "bun:test"

import { getInboxPriorityLabel, getInboxReasonLabel, getInboxStatusLabel } from "./labels.ts"

test("inbox labels expose daemon inbox terms without changing state", () => {
  expect(getInboxStatusLabel("replied")).toBe("Replied")
  expect(getInboxPriorityLabel("low")).toBe("Low priority")
  expect(getInboxReasonLabel("pull_request.updated")).toBe("Pull request updated")
})
