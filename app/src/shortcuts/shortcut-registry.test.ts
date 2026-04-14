import { expect, test, vi } from "bun:test"
import { createShortcuts } from "powerkeys"

const { AppCommand, onAppCommand } = await import("../commands/app-command.ts")
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

test("keydown dispatches one typed app command event", () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const listener = vi.fn()

  const unsubscribe = onAppCommand(AppCommand.navigation.openNewSessionDialog, listener)
  registry.applyKeymapSnapshot(
    "goddard",
    {
      "navigation.openNewSessionDialog": ["Alt+n"],
    },
    null,
  )

  try {
    dispatchKeydown(runtimeDocument, {
      key: "n",
      code: "KeyN",
      altKey: true,
    })

    expect(listener).toHaveBeenCalledWith({
      combo: "Alt+n",
      event: {
        key: "n",
        modifiers: {
          alt: true,
        },
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
        "navigation.openNewSessionDialog": ["Mod+Shift+n"],
        "navigation.openInbox": null,
      },
      null,
    )

    expect(registry.isHydrated).toBe(true)
    expect(registry.resolvedBindings["navigation.openNewSessionDialog"]).toEqual(["Mod+Shift+n"])
    expect(registry.resolvedBindings["navigation.openInbox"]).toBeUndefined()
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
