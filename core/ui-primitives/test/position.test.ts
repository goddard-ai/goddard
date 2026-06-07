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
  expect(floatingElement.style.boxSizing).toBe("border-box")

  stopPositioning()
  floatingElement.remove()
})

test("startFloatingPosition clamps oversized CSS bounds to the available size", async () => {
  const styleElement = document.createElement("style")
  styleElement.textContent = `
    .oversized-floating-menu {
      min-height: 100000px;
      min-width: 100000px;
    }
  `
  const floatingElement = document.createElement("div")
  floatingElement.className = "oversized-floating-menu"

  document.head.append(styleElement)
  document.body.append(floatingElement)

  const stopPositioning = startFloatingPosition({ x: 0, y: 0 }, floatingElement)
  await flushPositioning()

  expect(floatingElement.style.minHeight).toBe(
    floatingElement.style.getPropertyValue("--available-height"),
  )
  expect(floatingElement.style.minWidth).toBe(
    floatingElement.style.getPropertyValue("--available-width"),
  )
  expect(floatingElement.style.boxSizing).toBe("border-box")

  stopPositioning()
  floatingElement.remove()
  styleElement.remove()
})
