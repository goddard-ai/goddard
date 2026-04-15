import { expect, test } from "bun:test"
import { createShortcuts } from "powerkeys"

import { AppCommand, onAppCommand } from "~/commands/app-command.ts"
import { commandContext, isCommandAvailable } from "~/commands/command-context.ts"
import { ShortcutRegistry } from "./shortcut-registry.ts"

/** Creates one registry instance with an isolated document-like event boundary. */
function createTestRegistry() {
  const runtimeDocument = document.implementation.createHTMLDocument("shortcut-registry-test")
  const runtime = createShortcuts({
    target: runtimeDocument,
    editablePolicy: "ignore-editable",
    getActiveScopes: () => commandContext.activeScopes.peek(),
    onError: (error, info) => {
      console.error("Shortcut runtime error.", error, info)
    },
  })
  const registry = new ShortcutRegistry({
    runtime,
    bindingSet: runtime.createBindingSet(),
  })
  const cleanup = registry.setup()
  commandContext.activeScopes.value = []
  commandContext.activeTabKind.value = "main"
  commandContext.hasClosableActiveTab.value = false
  commandContext.selectedNavId.value = "inbox"
  commandContext.sessionInputActive.value = false
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
    registry.applyKeymapSnapshot("goddard", {}, null)

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
    registry.applyKeymapSnapshot(
      "goddard",
      {
        "navigation.openNewSessionDialog": ["Alt+n"],
      },
      null,
    )

    dispatchKeydown(runtimeDocument, {
      key: "n",
      code: "KeyN",
      altKey: true,
    })

    registry.applyKeymapSnapshot(
      "goddard",
      {
        "navigation.openNewSessionDialog": ["Shift+n"],
      },
      null,
    )

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

test("hasClosableActiveTab drives runtime availability for closeActiveTab", () => {
  const { registry, cleanup } = createTestRegistry()

  try {
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
    registry.applyKeymapSnapshot("goddard", {}, null)

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
