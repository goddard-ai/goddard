import { expect, test } from "bun:test"

import { createModalStack } from "./modal-stack.ts"

test("closeTopmost closes registered modals in last-opened order", () => {
  const stack = createModalStack()
  const closed: string[] = []

  const unregisterFirst = stack.register({
    id: "first",
    close() {
      closed.push("first")
    },
  })
  const unregisterSecond = stack.register({
    id: "second",
    close() {
      closed.push("second")
    },
  })

  try {
    expect(stack.hasOpenModal.value).toBe(true)
    expect(stack.closeTopmost()).toBe(true)
    expect(closed).toEqual(["second"])
    expect(stack.hasOpenModal.value).toBe(true)

    expect(stack.closeTopmost()).toBe(true)
    expect(closed).toEqual(["second", "first"])
    expect(stack.hasOpenModal.value).toBe(false)
  } finally {
    unregisterSecond()
    unregisterFirst()
  }
})

test("closeTopmost returns false when no modal is open", () => {
  const stack = createModalStack()

  expect(stack.closeTopmost()).toBe(false)
  expect(stack.hasOpenModal.value).toBe(false)
})

test("unregistering an open modal clears modal availability", () => {
  const stack = createModalStack()
  const unregister = stack.register({
    id: "only",
    close() {},
  })

  expect(stack.hasOpenModal.value).toBe(true)

  unregister()

  expect(stack.hasOpenModal.value).toBe(false)
  expect(stack.closeTopmost()).toBe(false)
})
