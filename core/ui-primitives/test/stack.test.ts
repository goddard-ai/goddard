import { expect, test } from "bun:test"

import { createOverlayStack } from "../src/overlay/stack.ts"

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

test("register closes same-group overlays that are not ancestors", () => {
  const stack = createOverlayStack()
  const parent = document.createElement("section")
  const firstAnchor = document.createElement("button")
  const secondAnchor = document.createElement("button")
  const firstContent = document.createElement("div")
  const secondContent = document.createElement("div")
  const closed: string[] = []

  parent.append(firstAnchor, secondAnchor)

  stack.register({
    id: "parent",
    elements: [parent],
    group: "default",
    close() {
      closed.push("parent")
    },
  })
  stack.register({
    id: "first",
    elements: [firstAnchor, firstContent],
    group: "default",
    close() {
      closed.push("first")
    },
  })
  stack.register({
    id: "second",
    elements: [secondAnchor, secondContent],
    group: "default",
    close() {
      closed.push("second")
    },
  })

  expect(closed).toEqual(["first"])
  expect(stack.isTopmost("second")).toBe(true)
  expect(stack.entries.value.map((entry) => entry.id)).toEqual(["parent", "second"])
})

test("register leaves different groups and unmanaged overlays open", () => {
  const stack = createOverlayStack()
  const closed: string[] = []

  stack.register({
    id: "default",
    elements: [document.createElement("div")],
    group: "default",
    close() {
      closed.push("default")
    },
  })
  stack.register({
    id: "other",
    elements: [document.createElement("div")],
    group: "other",
    close() {
      closed.push("other")
    },
  })
  stack.register({
    id: "unmanaged",
    elements: [document.createElement("div")],
    group: null,
    close() {
      closed.push("unmanaged")
    },
  })

  expect(closed).toEqual([])
  expect(stack.entries.value.map((entry) => entry.id)).toEqual(["default", "other", "unmanaged"])
})
