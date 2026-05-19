import { expect, test } from "bun:test"

import { startOverlayDismissal } from "./dismissal.ts"
import { overlayStack } from "./stack.ts"

test("outside pointer dismissal ignores pointer events inside nested overlay content", () => {
  const dialog = document.createElement("section")
  const nestedPortal = document.createElement("div")
  const outside = document.createElement("button")
  const nestedButton = document.createElement("button")
  nestedPortal.append(nestedButton)
  document.body.append(dialog, nestedPortal, outside)
  let closeCount = 0

  const unregister = overlayStack.register({
    id: "dialog",
    elements: [dialog, nestedPortal],
  })
  const stop = startOverlayDismissal({
    id: "dialog",
    close(reason) {
      expect(reason).toBe("outside")
      closeCount++
    },
  })

  try {
    nestedButton.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }))
    expect(closeCount).toBe(0)

    outside.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }))
    expect(closeCount).toBe(1)
  } finally {
    stop()
    unregister()
  }
})

test("escape dismissal closes only the topmost overlay", () => {
  const closed: string[] = []
  const unregisterFirst = overlayStack.register({
    id: "first",
    elements: [document.createElement("div")],
  })
  const unregisterSecond = overlayStack.register({
    id: "second",
    elements: [document.createElement("div")],
  })
  const stopFirst = startOverlayDismissal({
    id: "first",
    close() {
      closed.push("first")
    },
  })
  const stopSecond = startOverlayDismissal({
    id: "second",
    close() {
      closed.push("second")
    },
  })

  try {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }))
    expect(closed).toEqual(["second"])
  } finally {
    stopSecond()
    stopFirst()
    unregisterSecond()
    unregisterFirst()
  }
})
