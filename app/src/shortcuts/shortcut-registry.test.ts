import { expect, test } from "bun:test"

import { AppCommand, onAppCommand } from "~/commands/app-command.ts"
import { commandContext, isCommandAvailable } from "~/commands/command-context.ts"
import { registerModalStackEntry } from "~/lib/modal-stack.ts"
import { ShortcutRegistry } from "./shortcut-registry.ts"

/** Creates one registry instance with an isolated document-like event boundary. */
function createTestRegistry() {
  const runtimeDocument = document.implementation.createHTMLDocument("shortcut-registry-test")
  const registry = new ShortcutRegistry(runtimeDocument)
  const cleanup = registry.setup()
  commandContext.activeScopes.value = []
  commandContext.activeTabKind.value = "main"
  commandContext.hasClosableActiveTab.value = false
  commandContext.selectedNavId.value = "inbox"
  commandContext.sessionInputActive.value = false
  commandContext.sessionInputHasAdapterSelector.value = false
  commandContext.sessionInputHasBranchSelector.value = false
  commandContext.sessionInputHasLocationSelector.value = false
  commandContext.sessionInputCanSubmit.value = false
  commandContext.sessionInputHasModelSelector.value = false
  commandContext.sessionInputHasProjectSelector.value = false
  commandContext.sessionInputHasThinkingLevel.value = false

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
  const matches: unknown[] = []

  const unsubscribe = onAppCommand(AppCommand.navigation.openNewSessionDialog, (match) => {
    matches.push(match)
  })
  registry.applyKeymapSnapshot("goddard", {
    "navigation.openNewSessionDialog": ["Alt+n"],
  })

  try {
    dispatchKeydown(runtimeDocument, {
      key: "n",
      code: "KeyN",
      altKey: true,
    })

    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
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

test("default keymap dispatches switch-project from Mod+o", () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []

  const unsubscribe = onAppCommand(AppCommand.navigation.openSwitchProject, (match) => {
    matches.push(match)
  })

  try {
    registry.applyKeymapSnapshot("goddard", {})

    dispatchKeydown(runtimeDocument, {
      key: "o",
      code: "KeyO",
      ctrlKey: true,
    })

    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      combo: "Ctrl+o",
      event: {
        key: "o",
        modifiers: {
          ctrl: true,
        },
      },
    })
  } finally {
    unsubscribe()
    cleanup()
  }
})

test("applyKeymapSnapshot replaces previous bindings instead of accumulating them", () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []

  const unsubscribe = onAppCommand(AppCommand.navigation.openNewSessionDialog, (match) => {
    matches.push(match)
  })

  try {
    registry.applyKeymapSnapshot("goddard", {
      "navigation.openNewSessionDialog": ["Alt+n"],
    })

    dispatchKeydown(runtimeDocument, {
      key: "n",
      code: "KeyN",
      altKey: true,
    })

    registry.applyKeymapSnapshot("goddard", {
      "navigation.openNewSessionDialog": ["Shift+n"],
    })

    dispatchKeydown(runtimeDocument, {
      key: "n",
      code: "KeyN",
      altKey: true,
    })

    dispatchKeydown(runtimeDocument, {
      key: "N",
      code: "KeyN",
      shiftKey: true,
    })

    expect(matches).toHaveLength(2)
    expect(matches.map((match) => (match as { combo: string }).combo)).toEqual(["Alt+n", "Shift+n"])
  } finally {
    unsubscribe()
    cleanup()
  }
})

test("applyKeymapSnapshot resolves overrides into the live keymap snapshot", () => {
  const { registry, cleanup } = createTestRegistry()

  try {
    registry.applyKeymapSnapshot("goddard", {
      "navigation.openNewSessionDialog": ["Mod+Shift+n"],
      "navigation.openInbox": null,
    })

    expect(registry.resolvedBindings["navigation.openNewSessionDialog"]).toEqual(["Mod+Shift+n"])
    expect(registry.resolvedBindings["navigation.openInbox"]).toBeUndefined()
  } finally {
    cleanup()
  }
})

test("command-owned when clauses gate both dispatch and palette availability", () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []

  const unsubscribe = onAppCommand(AppCommand.workbench.closeActiveTab, (match) => {
    matches.push(match)
  })

  try {
    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(false)

    dispatchKeydown(runtimeDocument, {
      key: "w",
      code: "KeyW",
      ctrlKey: true,
    })

    expect(matches).toEqual([])

    commandContext.hasClosableActiveTab.value = true

    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(true)

    dispatchKeydown(runtimeDocument, {
      key: "w",
      code: "KeyW",
      ctrlKey: true,
    })

    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      combo: "Ctrl+w",
      event: {
        key: "w",
        modifiers: {
          ctrl: true,
        },
      },
    })
  } finally {
    unsubscribe()
    cleanup()
  }
})

test("modal or closable tab drives runtime availability for closeActiveTab", () => {
  const { registry, cleanup } = createTestRegistry()

  try {
    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(false)

    const unregisterModal = registerModalStackEntry({
      id: "shortcut-registry-test:modal",
      close() {},
    })

    try {
      expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(true)
    } finally {
      unregisterModal()
    }

    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(false)

    commandContext.hasClosableActiveTab.value = true

    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(true)
  } finally {
    cleanup()
  }
})

test("active shortcut scopes drive availability checks", () => {
  const { registry, cleanup } = createTestRegistry()

  try {
    expect(isCommandAvailable(registry.runtime, { scope: "editor" })).toBe(false)

    commandContext.activeScopes.value = ["editor"]

    expect(isCommandAvailable(registry.runtime, { scope: "editor" })).toBe(true)
  } finally {
    cleanup()
  }
})

test("session input context lets launch-dialog selectors override the global palette binding", () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const paletteMatches: unknown[] = []
  const projectMatches: unknown[] = []

  const stopPalette = onAppCommand(AppCommand.navigation.openCommandPalette, (match) => {
    paletteMatches.push(match)
  })
  const stopProject = onAppCommand(AppCommand.sessionInput.openProjectSelector, (match) => {
    projectMatches.push(match)
  })

  try {
    registry.applyKeymapSnapshot("goddard", {})

    dispatchKeydown(runtimeDocument, {
      key: "p",
      code: "KeyP",
      ctrlKey: true,
    })

    expect(paletteMatches).toHaveLength(1)
    expect(projectMatches).toHaveLength(0)

    commandContext.sessionInputActive.value = true
    commandContext.sessionInputHasProjectSelector.value = true

    dispatchKeydown(runtimeDocument, {
      key: "p",
      code: "KeyP",
      ctrlKey: true,
    })

    expect(paletteMatches).toHaveLength(1)
    expect(projectMatches).toHaveLength(1)
  } finally {
    stopPalette()
    stopProject()
    cleanup()
  }
})

test("addCommandBinding updates overrides for commands without built-in defaults", async () => {
  const { registry, cleanup } = createTestRegistry()

  try {
    registry.applyKeymapSnapshot("goddard", {})

    expect(
      await registry.addCommandBinding(AppCommand.navigation.openKeyboardShortcuts.id, "Mod+/"),
    ).toBe(true)
    expect(registry.resolvedBindings[AppCommand.navigation.openKeyboardShortcuts.id]).toEqual([
      "Mod+/",
    ])
    expect(registry.overrides[AppCommand.navigation.openKeyboardShortcuts.id]).toEqual(["Mod+/"])
  } finally {
    cleanup()
  }
})

test("updateCommandBindingWhen promotes and collapses binding-local when overrides", async () => {
  const { registry, cleanup } = createTestRegistry()

  try {
    registry.applyKeymapSnapshot("goddard", {})

    expect(
      await registry.updateCommandBindingWhen(
        AppCommand.navigation.openCommandPalette.id,
        0,
        "workbench.hasClosableActiveTab",
      ),
    ).toBe(true)
    expect(registry.resolvedBindings[AppCommand.navigation.openCommandPalette.id]).toEqual([
      {
        combo: "Mod+p",
        when: "workbench.hasClosableActiveTab",
      },
    ])

    expect(
      await registry.updateCommandBindingWhen(
        AppCommand.navigation.openCommandPalette.id,
        0,
        AppCommand.navigation.openCommandPalette.when ?? null,
      ),
    ).toBe(true)
    expect(registry.resolvedBindings[AppCommand.navigation.openCommandPalette.id]).toEqual([
      "Mod+p",
    ])
  } finally {
    cleanup()
  }
})
