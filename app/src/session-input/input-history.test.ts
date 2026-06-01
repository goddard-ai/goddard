import { expect, test } from "bun:test"

import { getSessionInputPromptHistoryIndexes, getSessionInputPromptText } from "./input-history.ts"

test("getSessionInputPromptText ignores resource link metadata", () => {
  expect(
    getSessionInputPromptText([
      { type: "text", text: "Review " },
      {
        type: "resource_link",
        name: "checkout.ts",
        uri: "file:///repo/checkout.ts",
        title: "Checkout",
        description: "Checkout flow",
      },
      { type: "text", text: " carefully" },
    ]),
  ).toBe("Review  carefully")
})

test("getSessionInputPromptHistoryIndexes filters by case-insensitive text substring", () => {
  const promptHistory = [
    [{ type: "text", text: "Fix checkout loading" }],
    [
      { type: "text", text: "Review " },
      {
        type: "resource_link",
        name: "checkout.ts",
        uri: "file:///repo/checkout.ts",
        title: "Checkout",
        description: "Checkout flow",
      },
    ],
    [{ type: "text", text: "Add billing tests" }],
  ]

  expect(getSessionInputPromptHistoryIndexes(promptHistory, "CHECK")).toEqual([0])
  expect(getSessionInputPromptHistoryIndexes(promptHistory, "checkout.ts")).toEqual([])
  expect(getSessionInputPromptHistoryIndexes(promptHistory, "")).toEqual([0, 1, 2])
})
