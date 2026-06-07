import { expect, test } from "bun:test"

import { getNextKeyboardRowIndex } from "./row-keyboard-navigation.ts"

test("row keyboard navigation starts at the nearest boundary", () => {
  expect(getNextKeyboardRowIndex({ currentIndex: null, direction: 1, rowCount: 3 })).toBe(0)
  expect(getNextKeyboardRowIndex({ currentIndex: -1, direction: -1, rowCount: 3 })).toBe(2)
})

test("row keyboard navigation wraps through visible rows", () => {
  expect(getNextKeyboardRowIndex({ currentIndex: 0, direction: 1, rowCount: 3 })).toBe(1)
  expect(getNextKeyboardRowIndex({ currentIndex: 2, direction: 1, rowCount: 3 })).toBe(0)
  expect(getNextKeyboardRowIndex({ currentIndex: 0, direction: -1, rowCount: 3 })).toBe(2)
})

test("row keyboard navigation ignores empty lists", () => {
  expect(getNextKeyboardRowIndex({ currentIndex: null, direction: 1, rowCount: 0 })).toBeNull()
})
