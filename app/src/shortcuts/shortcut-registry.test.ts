import { expect, test } from "bun:test"

import { AppCommand } from "~/commands/app-command.ts"
import { CommandContext, isCommandAvailable } from "~/commands/command-context.ts"
import { MainTab } from "~/main-tab.ts"
import { WorkbenchTabSet } from "~/workbench-tab-set.ts"
import { ShortcutRegistry } from "./shortcut-registry.ts"

/** Creates one registry instance with isolated command context state. */
function createTestRegistry() {
  const runtimeDocument = document.implementation.createHTMLDocument("shortcut-registry-test")
  const commandContext = new CommandContext({
    mainTab: new MainTab(),
    target: runtimeDocument,
    workbenchTabSet: new WorkbenchTabSet(),
  })
  const registry = new ShortcutRegistry({
    runtime: commandContext.runtime,
  })
  const cleanupCommandContext = commandContext.setup()
  const cleanupRegistry = registry.setup()

  return {
    commandContext,
    registry,
    cleanup() {
      cleanupRegistry()
      cleanupCommandContext()
    },
  }
}

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

test("active shortcut scopes drive availability checks", () => {
  const { commandContext, registry, cleanup } = createTestRegistry()

  try {
    expect(isCommandAvailable(registry.runtime, { scope: "editor" })).toBe(false)

    commandContext.setActiveScopes(["editor"])

    expect(isCommandAvailable(registry.runtime, { scope: "editor" })).toBe(true)
  } finally {
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
