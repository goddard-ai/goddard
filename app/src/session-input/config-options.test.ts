import { expect, test } from "vitest"

import { stepConfigOptionValue, type SessionConfigOption } from "./config-options.ts"

test("stepConfigOptionValue clamps select options in their declared order", () => {
  const option = {
    id: "thinking",
    name: "Thinking level",
    category: "thought_level",
    type: "select",
    currentValue: "medium",
    options: [
      { value: "low", name: "Low" },
      { value: "medium", name: "Medium" },
      { value: "high", name: "High" },
    ],
  } satisfies SessionConfigOption

  expect(stepConfigOptionValue(option, "medium", -1)).toBe("low")
  expect(stepConfigOptionValue(option, "medium", 1)).toBe("high")
  expect(stepConfigOptionValue(option, "low", -1)).toBe("low")
  expect(stepConfigOptionValue(option, "high", 1)).toBe("high")
})

test("stepConfigOptionValue maps boolean options to off and on", () => {
  const option = {
    id: "thinking",
    name: "Thinking level",
    category: "thought_level",
    type: "boolean",
    currentValue: false,
  } satisfies SessionConfigOption

  expect(stepConfigOptionValue(option, true, -1)).toBe(false)
  expect(stepConfigOptionValue(option, false, 1)).toBe(true)
})
