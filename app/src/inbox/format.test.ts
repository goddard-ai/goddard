import { expect, test } from "vitest"

import { formatInboxUpdatedTime } from "./format.ts"

test("compact updated time labels use minutes, hours, days, and dates", () => {
  const now = Date.UTC(2026, 0, 10, 12, 0)

  expect(formatInboxUpdatedTime(now, now)).toBe("now")
  expect(formatInboxUpdatedTime(now - 5 * 60_000, now)).toBe("5m")
  expect(formatInboxUpdatedTime(now - 2 * 60 * 60_000, now)).toBe("2h")
  expect(formatInboxUpdatedTime(now - 3 * 24 * 60 * 60_000, now)).toBe("3d")
  expect(formatInboxUpdatedTime(now - 8 * 24 * 60 * 60_000, now)).toBe("Jan 2")
})
