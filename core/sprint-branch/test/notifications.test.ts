import { describe, expect, test } from "bun:test"

import { reviewReadyNotificationOptions } from "../src/notifications"

describe("review ready notifications", () => {
  const notification = {
    sprint: "example",
    task: "010-task-name",
    reviewBranch: "sprint/example/review",
  }

  test("uses Heroine sound on macOS", () => {
    expect(reviewReadyNotificationOptions(notification, "darwin").sound).toBe("Heroine")
  })

  test("uses the default sound on other platforms", () => {
    expect(reviewReadyNotificationOptions(notification, "linux").sound).toBe(true)
  })
})
