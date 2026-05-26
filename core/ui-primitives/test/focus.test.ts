import { expect, test } from "bun:test"

import { keepFocusInside, restoreFocus } from "../src/overlay/focus.ts"

test("restoreFocus focuses an attached element", () => {
  const button = document.createElement("button")
  document.body.append(button)

  restoreFocus(button)

  expect(document.activeElement).toBe(button)
})

test("keepFocusInside loops tab focus from the last element to the first", () => {
  const container = document.createElement("div")
  const first = document.createElement("button")
  const last = document.createElement("button")
  container.append(first, last)
  document.body.append(container)
  last.focus()

  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Tab" })
  keepFocusInside(container, event)

  expect(event.defaultPrevented).toBe(true)
  expect(document.activeElement).toBe(first)
})
