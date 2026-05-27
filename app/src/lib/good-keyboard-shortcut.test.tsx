import { expect, test } from "bun:test"
import { render } from "preact"

import GoodKeyboardShortcut from "./good-keyboard-shortcut.tsrx"

function withPlatform(platform: string, runTest: () => void) {
  const platformDescriptor = Object.getOwnPropertyDescriptor(navigator, "platform")
  const userAgentDataDescriptor = Object.getOwnPropertyDescriptor(navigator, "userAgentData")

  Object.defineProperty(navigator, "platform", {
    configurable: true,
    value: platform,
  })
  Object.defineProperty(navigator, "userAgentData", {
    configurable: true,
    value: { platform },
  })

  try {
    runTest()
  } finally {
    if (platformDescriptor) {
      Object.defineProperty(navigator, "platform", platformDescriptor)
    } else {
      delete (navigator as { platform?: string }).platform
    }

    if (userAgentDataDescriptor) {
      Object.defineProperty(navigator, "userAgentData", userAgentDataDescriptor)
    } else {
      delete (navigator as { userAgentData?: unknown }).userAgentData
    }
  }
}

test("GoodKeyboardShortcut renders macOS symbols for non-character keys", () => {
  const container = document.createElement("div")

  withPlatform("MacIntel", () => {
    render(<GoodKeyboardShortcut expression="Mod+Shift+Enter" />, container)

    expect(container.textContent).toBe("⌘⇧↩")
    expect(container.querySelector("[aria-label='Mod+Shift+Enter']")).not.toBeNull()

    render(null, container)
  })
})

test("GoodKeyboardShortcut renders Mod as Ctrl off Apple platforms", () => {
  const container = document.createElement("div")

  withPlatform("Win32", () => {
    render(<GoodKeyboardShortcut expression="Mod+Shift+Enter" />, container)

    expect(container.textContent).toBe("CtrlShift↩")

    render(null, container)
  })
})

test("GoodKeyboardShortcut renders character keys in monospace spans", () => {
  const container = document.createElement("div")

  withPlatform("MacIntel", () => {
    render(<GoodKeyboardShortcut expression="Alt+/" />, container)

    expect(container.textContent).toBe("⌥/")
    expect(container.querySelector("[data-key-kind='character']")?.textContent).toBe("/")

    render(null, container)
  })
})

test("GoodKeyboardShortcut renders space-separated sequences", () => {
  const container = document.createElement("div")

  withPlatform("MacIntel", () => {
    render(<GoodKeyboardShortcut expression="Mod+k Mod+p" />, container)

    expect(container.textContent).toBe("⌘Kthen⌘P")

    render(null, container)
  })
})
