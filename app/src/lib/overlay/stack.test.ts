import { expect, test } from "bun:test"

import { createOverlayStack } from "./stack.ts"

test("contains treats nested portal content as inside active overlays", () => {
  const stack = createOverlayStack()
  const dialog = document.createElement("section")
  const menuPortal = document.createElement("div")
  const menuItem = document.createElement("button")
  menuPortal.append(menuItem)
  document.body.append(dialog, menuPortal)

  const unregister = stack.register({
    id: "dialog",
    elements: [dialog, menuPortal],
  })

  try {
    expect(stack.contains(menuItem)).toBe(true)
  } finally {
    unregister()
  }
})

test("closeTopmost only closes the last registered closable overlay", () => {
  const stack = createOverlayStack()
  const closed: string[] = []

  stack.register({
    id: "first",
    elements: [document.createElement("div")],
    close() {
      closed.push("first")
    },
  })
  stack.register({
    id: "second",
    elements: [document.createElement("div")],
    close() {
      closed.push("second")
    },
  })

  expect(stack.isTopmost("second")).toBe(true)
  expect(stack.closeTopmost()).toBe(true)
  expect(closed).toEqual(["second"])
  expect(stack.isTopmost("first")).toBe(true)
})
