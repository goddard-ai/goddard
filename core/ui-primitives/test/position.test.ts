import { expect, test } from "bun:test"

import { startFloatingPosition } from "../src/overlay/position.ts"

async function flushPositioning() {
  await new Promise((resolve) => window.setTimeout(resolve, 0))
}

test("startFloatingPosition writes available size variables as CSS custom properties", async () => {
  const floatingElement = document.createElement("div")

  document.body.append(floatingElement)

  const stopPositioning = startFloatingPosition({ x: 0, y: 0 }, floatingElement)
  await flushPositioning()

  expect(floatingElement.style.getPropertyValue("--available-height")).toEndWith("px")
  expect(floatingElement.style.getPropertyValue("--available-width")).toEndWith("px")
  expect(floatingElement.style.getPropertyValue("--reference-width")).toEndWith("px")

  stopPositioning()
  floatingElement.remove()
})
