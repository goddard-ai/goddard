import { expect, test } from "bun:test"

import { orderSelectorItemsByUsage, SelectorUsage } from "./selector-usage.ts"

test("orderSelectorItemsByUsage puts selected first, then recent values, then natural order", () => {
  const items = [
    { value: "alpha", label: "Alpha" },
    { value: "beta", label: "Beta" },
    { value: "gamma", label: "Gamma" },
    { value: "delta", label: "Delta" },
  ]

  expect(
    orderSelectorItemsByUsage(items, {
      recentUsedValues: ["gamma", "missing", "beta"],
      selectedValue: "delta",
    }).map((item) => item.value),
  ).toEqual(["delta", "gamma", "beta", "alpha"])
})

test("orderSelectorItemsByUsage does not duplicate selected recent values", () => {
  const items = [
    { value: "alpha", label: "Alpha" },
    { value: "beta", label: "Beta" },
    { value: "gamma", label: "Gamma" },
  ]

  expect(
    orderSelectorItemsByUsage(items, {
      recentUsedValues: ["beta", "gamma"],
      selectedValue: "beta",
    }).map((item) => item.value),
  ).toEqual(["beta", "gamma", "alpha"])
})

test("SelectorUsage separates current selection from recent use", () => {
  const selectorUsage = new SelectorUsage()

  selectorUsage.setCurrentValue("session.control.model:agent-a", "model-a")

  expect(selectorUsage.getCurrentValue("session.control.model:agent-a")).toBe("model-a")
  expect(selectorUsage.getRecentUsedValues("session.control.model:agent-a")).toEqual([])

  selectorUsage.recordUsedValue("session.control.model:agent-a", "model-b")

  expect(selectorUsage.getCurrentValue("session.control.model:agent-a")).toBe("model-b")
  expect(selectorUsage.getRecentUsedValues("session.control.model:agent-a")).toEqual(["model-b"])
})
