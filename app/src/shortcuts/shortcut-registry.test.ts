import { expect, test } from "bun:test"
import { createShortcuts } from "powerkeys"

const { ShortcutCommands } = await import("../shared/shortcut-keymap.ts")
const { ShortcutRegistry } = await import("./shortcut-registry.ts")

/** Creates one registry instance with an isolated document-like event boundary. */
function createTestRegistry() {
  const runtimeDocument = document.implementation.createHTMLDocument("shortcut-registry-test")
  const registry = new ShortcutRegistry({
    runtime: createShortcuts({ target: runtimeDocument }),
  })
  const cleanup = registry.setup()

  return {
    registry,
    runtimeDocument,
    cleanup,
  }
}

/** Dispatches one synthetic keydown event through the test shortcut boundary. */
function dispatchKeydown(target: EventTarget, init: KeyboardEventInit) {
  target.dispatchEvent(
    new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      ...init,
    }),
  )
}

test("keydown emits one typed shortcut command event", () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const events: Array<{
    source: "keyboard" | "native-menu" | "programmatic"
    match?: {
      combo: string
      event: {
        key: string
        modifiers: {
          alt: boolean
        }
      }
      matchedScope: string
    }
  }> = []

  const unsubscribe = registry.on(ShortcutCommands.newSession, (detail) => {
    events.push(detail)
  })
  registry.applyKeymapSnapshot(
    "goddard",
    {
      [ShortcutCommands.newSession]: ["Alt+n"],
    },
    null,
  )

  try {
    dispatchKeydown(runtimeDocument, {
      key: "n",
      code: "KeyN",
      altKey: true,
    })

    expect(events).toHaveLength(1)
    expect(events[0]).toMatchObject({
      source: "keyboard",
      match: {
        combo: "Alt+n",
        event: {
          key: "n",
          modifiers: {
            alt: true,
          },
        },
        matchedScope: "root",
      },
    })
  } finally {
    unsubscribe()
    cleanup()
  }
})

test("applyKeymapSnapshot resolves overrides into the live keymap snapshot", () => {
  const { registry, cleanup } = createTestRegistry()

  try {
    registry.applyKeymapSnapshot(
      "goddard",
      {
        [ShortcutCommands.newSession]: ["Mod+Shift+n"],
        [ShortcutCommands.openInbox]: null,
      },
      null,
    )

    expect(registry.isHydrated).toBe(true)
    expect(registry.resolvedBindings[ShortcutCommands.newSession]).toEqual(["Mod+Shift+n"])
    expect(registry.resolvedBindings[ShortcutCommands.openInbox]).toBeUndefined()
  } finally {
    cleanup()
  }
})

test("syncWorkbenchContext and syncOverlayContext track the initial runtime context surface", () => {
  const { registry, cleanup } = createTestRegistry()

  try {
    registry.syncWorkbenchContext({
      activeTabKind: "main",
      hasClosableActiveTab: false,
      selectedNavId: "sessions",
    })
    registry.syncOverlayContext({
      isOpen: true,
      kind: "shortcut-capture",
    })

    expect(registry.activeTabKind).toBe("main")
    expect(registry.hasClosableActiveTab).toBe(false)
    expect(registry.selectedNavId).toBe("sessions")
    expect(registry.overlayIsOpen).toBe(true)
    expect(registry.overlayKind).toBe("shortcut-capture")
  } finally {
    cleanup()
  }
})
