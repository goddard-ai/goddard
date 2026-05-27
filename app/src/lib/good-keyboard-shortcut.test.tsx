import { expect, test } from "bun:test"
import { render } from "preact"

import GoodKeyboardShortcut from "./good-keyboard-shortcut.tsrx"

test("GoodKeyboardShortcut renders macOS symbols for non-character keys", () => {
  const container = document.createElement("div")

  render(<GoodKeyboardShortcut expression="Mod+Shift+Enter" />, container)

  expect(container.textContent).toBe("⌘⇧↩")
  expect(container.querySelector("[aria-label='Mod+Shift+Enter']")).not.toBeNull()

  render(null, container)
})

test("GoodKeyboardShortcut renders character keys in monospace spans", () => {
  const container = document.createElement("div")

  render(<GoodKeyboardShortcut expression="Alt+/" />, container)

  expect(container.textContent).toBe("⌥/")
  expect(container.querySelector("[data-key-kind='character']")?.textContent).toBe("/")

  render(null, container)
})

test("GoodKeyboardShortcut renders space-separated sequences", () => {
  const container = document.createElement("div")

  render(<GoodKeyboardShortcut expression="Mod+k Mod+p" />, container)

  expect(container.textContent).toBe("⌘Kthen⌘P")

  render(null, container)
})
