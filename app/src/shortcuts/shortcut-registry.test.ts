import { expect, test, vi } from "bun:test"

vi.mock("~/desktop-host.ts", () => ({
  desktopHost: {
    readShortcutKeymap: vi.fn(async () => ({ keymap: null, error: null })),
    writeShortcutKeymap: vi.fn(),
  },
}))

const { ShortcutCommands } = await import("../shared/shortcut-keymap.ts")
const { ShortcutRegistry } = await import("./shortcut-registry.ts")

test("dispatch emits one typed shortcut command event", () => {
  const registry = new ShortcutRegistry()
  const listener = vi.fn()

  const unsubscribe = registry.on(ShortcutCommands.newSession, listener)

  registry.dispatch(ShortcutCommands.newSession, { source: "programmatic" })

  expect(listener).toHaveBeenCalledWith({
    commandId: ShortcutCommands.newSession,
    source: "programmatic",
  })

  unsubscribe()
})

test("applyKeymapSnapshot resolves overrides before the runtime is set up", () => {
  const registry = new ShortcutRegistry()

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
})

test("syncWorkbenchContext and syncOverlayContext track the initial runtime context surface", () => {
  const registry = new ShortcutRegistry()

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
})
