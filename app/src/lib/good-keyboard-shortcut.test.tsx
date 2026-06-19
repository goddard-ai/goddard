import { render } from "preact"
import { expect, test } from "vitest"

import { GoodKeyboardShortcut } from "./good-keyboard-shortcut.tsrx"

function withPlatform(platform: string, runTest: () => void) {
  const userAgentDataDescriptor = Object.getOwnPropertyDescriptor(navigator, "userAgentData")

  Object.defineProperty(navigator, "userAgentData", {
    configurable: true,
    value: { platform },
  })

  try {
    runTest()
  } finally {
    if (userAgentDataDescriptor) {
      Object.defineProperty(navigator, "userAgentData", userAgentDataDescriptor)
    } else {
      delete (navigator as { userAgentData?: unknown }).userAgentData
    }
  }
}

test("GoodKeyboardShortcut renders macOS symbols for non-character keys", () => {
  const container = document.createElement("div")

  withPlatform("macOS", () => {
    render(<GoodKeyboardShortcut expression="Mod+Shift+Enter" />, container)

    expect(container.textContent).toBe("⌘⇧↩")
    expect(container.querySelector("[aria-label='Mod+Shift+Enter']")).not.toBeNull()

    render(null, container)
  })
})

test("GoodKeyboardShortcut renders Mod as Ctrl off Apple platforms", () => {
  const container = document.createElement("div")

  withPlatform("Windows", () => {
    render(<GoodKeyboardShortcut expression="Mod+Shift+Enter" />, container)

    expect(container.textContent).toBe("CtrlShift↩")

    render(null, container)
  })
})

test("GoodKeyboardShortcut renders character keys in monospace spans", () => {
  const container = document.createElement("div")

  withPlatform("macOS", () => {
    render(<GoodKeyboardShortcut expression="Alt+/" />, container)

    expect(container.textContent).toBe("⌥/")
    expect(container.querySelector("[data-key-kind='character']")?.textContent).toBe("/")

    render(null, container)
  })
})

test("GoodKeyboardShortcut renders physical digit codes as numerals", () => {
  const container = document.createElement("div")

  withPlatform("macOS", () => {
    render(<GoodKeyboardShortcut expression="Alt+Digit1" />, container)

    expect(container.textContent).toBe("⌥1")
    expect(container.querySelector("[data-key-kind='character']")?.textContent).toBe("1")

    render(null, container)
  })
})

test("GoodKeyboardShortcut renders space-separated sequences", () => {
  const container = document.createElement("div")

  withPlatform("macOS", () => {
    render(<GoodKeyboardShortcut expression="Mod+k Mod+p" />, container)

    expect(container.textContent).toBe("⌘Kthen⌘P")

    render(null, container)
  })
})
