import { expect, test } from "bun:test"
import { Fragment, h, render } from "preact"

import { MainTab } from "~/main-tab.ts"
import { ShortcutRegistry } from "~/shortcuts/shortcut-registry.ts"
import { WorkbenchTabSet } from "~/workbench-tab-set.ts"
import { AppCommand, useAppCommand } from "./app-command.ts"
import { CommandContext, isCommandAvailable, isCommandPaletteVisible } from "./command-context.ts"
import { CommandLayerProvider } from "./command-layer.tsrx"

/** Creates one registry instance with an isolated document-like event boundary. */
function createTestRegistry() {
  const runtimeDocument = document.implementation.createHTMLDocument("app-command-test")
  const mainTab = new MainTab()
  const workbenchTabSet = new WorkbenchTabSet()
  const commandContext = new CommandContext({
    mainTab,
    target: runtimeDocument,
    workbenchTabSet,
  })
  const registry = new ShortcutRegistry({
    runtime: commandContext.runtime,
  })
  const cleanupCommandContext = commandContext.setup()
  const cleanupRegistry = registry.setup()

  return {
    commandContext,
    registry,
    runtimeDocument,
    workbenchTabSet,
    cleanup() {
      cleanupRegistry()
      cleanupCommandContext()
    },
  }
}

/** Dispatches one synthetic keydown event through the test shortcut boundary. */
function dispatchKeydown(target: EventTarget, init: KeyboardEventInit) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    ...init,
  })

  target.dispatchEvent(event)
  return event
}

async function flushRenderEffects() {
  await Promise.resolve()
  await new Promise((resolve) => {
    window.setTimeout(resolve, 0)
  })
  await new Promise((resolve) => {
    window.setTimeout(resolve, 0)
  })
}

function TestCommandHandler(props: {
  active?: boolean
  command: AppCommand
  onMatch: (match?: unknown) => void
}) {
  useAppCommand(props.command, props.onMatch, { active: props.active })
  return null
}

function TestLayeredCommands(props: {
  dialogActive: boolean
  onSwitchProject: (match?: unknown) => void
  onProject: (match?: unknown) => void
  projectActive: boolean
}) {
  return h(
    Fragment,
    {},
    h(TestCommandHandler, {
      command: AppCommand.navigation.openSwitchProject,
      onMatch: props.onSwitchProject,
    }),
    h(
      CommandLayerProvider,
      { active: props.dialogActive },
      h(TestCommandHandler, {
        active: props.projectActive,
        command: AppCommand.sessionInput.openProjectSelector,
        onMatch: props.onProject,
      }),
    ),
  )
}

test("app commands expose stable ids and shortcut groups", () => {
  expect(AppCommand.sessionChat.completeSession.id).toBe("sessionChat.completeSession")
  expect(AppCommand.sessionChat.completeSession.group).toBe("session")
  expect(AppCommand.sessionInput.openModelSelector.group).toBe("session")
  expect(AppCommand.workbench.closeActiveTab.group).toBe("workbench")
})

test("keydown dispatches one typed app command event", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  render(
    h(TestCommandHandler, {
      command: AppCommand.navigation.openNewSessionDialog,
      onMatch(match) {
        matches.push(match)
      },
    }),
    container,
  )

  try {
    await flushRenderEffects()
    registry.applyKeymapSnapshot("goddard", {
      "navigation.openNewSessionDialog": ["Alt+n"],
    })

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
    render(null, container)
    cleanup()
  }
})

test("meta app command bindings dispatch from editable targets", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  const editable = runtimeDocument.createElement("div")
  editable.contentEditable = "true"
  runtimeDocument.body.append(container, editable)

  render(
    h(TestCommandHandler, {
      command: AppCommand.navigation.openNewSessionDialog,
      onMatch(match) {
        matches.push(match)
      },
    }),
    container,
  )

  try {
    await flushRenderEffects()
    registry.applyKeymapSnapshot("goddard", {})

    dispatchKeydown(editable, {
      key: "n",
      code: "KeyN",
      ctrlKey: true,
    })

    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      combo: "Ctrl+n",
    })
  } finally {
    render(null, container)
    cleanup()
  }
})

test("default keymap dispatches switch-project from Mod+o", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  render(
    h(TestCommandHandler, {
      command: AppCommand.navigation.openSwitchProject,
      onMatch(match) {
        matches.push(match)
      },
    }),
    container,
  )

  try {
    await flushRenderEffects()
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
    render(null, container)
    cleanup()
  }
})

test("default keymap dispatches main navigation from physical Alt number keys", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  render(
    h(TestCommandHandler, {
      command: AppCommand.navigation.openInbox,
      onMatch(match) {
        matches.push(match)
      },
    }),
    container,
  )

  try {
    await flushRenderEffects()
    registry.applyKeymapSnapshot("goddard", {})

    dispatchKeydown(runtimeDocument, {
      key: "¡",
      code: "Digit1",
      altKey: true,
    })

    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      combo: "Alt+Digit1",
      event: {
        code: "Digit1",
        modifiers: {
          alt: true,
        },
      },
    })
  } finally {
    render(null, container)
    cleanup()
  }
})

test("default keymap dispatches settings from Mod+,", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  render(
    h(TestCommandHandler, {
      command: AppCommand.navigation.openSettings,
      onMatch(match) {
        matches.push(match)
      },
    }),
    container,
  )

  try {
    await flushRenderEffects()
    registry.applyKeymapSnapshot("goddard", {})

    dispatchKeydown(runtimeDocument, {
      key: ",",
      code: "Comma",
      ctrlKey: true,
    })

    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      combo: "Ctrl+,",
      event: {
        key: ",",
        modifiers: {
          ctrl: true,
        },
      },
    })
  } finally {
    render(null, container)
    cleanup()
  }
})

test("default keymap dispatches session chat prompt navigation from Mod+ArrowUp and Mod+ArrowDown", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const previousMatches: unknown[] = []
  const nextMatches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  render(
    h(
      Fragment,
      {},
      h(TestCommandHandler, {
        command: AppCommand.sessionChat.skipToPreviousPrompt,
        onMatch(match) {
          previousMatches.push(match)
        },
      }),
      h(TestCommandHandler, {
        command: AppCommand.sessionChat.skipToNextPrompt,
        onMatch(match) {
          nextMatches.push(match)
        },
      }),
    ),
    container,
  )

  try {
    await flushRenderEffects()
    registry.applyKeymapSnapshot("goddard", {})

    dispatchKeydown(runtimeDocument, {
      key: "ArrowUp",
      code: "ArrowUp",
      ctrlKey: true,
    })
    dispatchKeydown(runtimeDocument, {
      key: "ArrowDown",
      code: "ArrowDown",
      ctrlKey: true,
    })

    expect(previousMatches).toHaveLength(1)
    expect(previousMatches[0]).toMatchObject({
      combo: "Ctrl+ArrowUp",
      event: {
        key: "ArrowUp",
        modifiers: {
          ctrl: true,
        },
      },
    })
    expect(nextMatches).toHaveLength(1)
    expect(nextMatches[0]).toMatchObject({
      combo: "Ctrl+ArrowDown",
      event: {
        key: "ArrowDown",
        modifiers: {
          ctrl: true,
        },
      },
    })
  } finally {
    render(null, container)
    cleanup()
  }
})

test("default keymap dispatches session chat actions from Alt+Shift+G and Mod+Shift+Enter", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const changeMatches: unknown[] = []
  const completeMatches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  render(
    h(
      Fragment,
      {},
      h(TestCommandHandler, {
        command: AppCommand.sessionChat.viewChanges,
        onMatch(match) {
          changeMatches.push(match)
        },
      }),
      h(TestCommandHandler, {
        command: AppCommand.sessionChat.completeSession,
        onMatch(match) {
          completeMatches.push(match)
        },
      }),
    ),
    container,
  )

  try {
    await flushRenderEffects()
    registry.applyKeymapSnapshot("goddard", {})

    dispatchKeydown(runtimeDocument, {
      key: "G",
      code: "KeyG",
      altKey: true,
      shiftKey: true,
    })
    dispatchKeydown(runtimeDocument, {
      key: "Enter",
      code: "Enter",
      ctrlKey: true,
      shiftKey: true,
    })

    expect(changeMatches).toHaveLength(1)
    expect(changeMatches[0]).toMatchObject({
      combo: "Alt+Shift+g",
      event: {
        key: "g",
        modifiers: {
          alt: true,
          shift: true,
        },
      },
    })
    expect(completeMatches).toHaveLength(1)
    expect(completeMatches[0]).toMatchObject({
      combo: "Ctrl+Shift+Enter",
      event: {
        key: "Enter",
        modifiers: {
          ctrl: true,
          shift: true,
        },
      },
    })
  } finally {
    render(null, container)
    cleanup()
  }
})

test("applyKeymapSnapshot replaces previous bindings instead of accumulating them", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  render(
    h(TestCommandHandler, {
      command: AppCommand.navigation.openNewSessionDialog,
      onMatch(match) {
        matches.push(match)
      },
    }),
    container,
  )

  try {
    await flushRenderEffects()
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
    render(null, container)
    cleanup()
  }
})

test("command-owned when clauses gate both dispatch and palette availability", async () => {
  const { registry, runtimeDocument, workbenchTabSet, cleanup } = createTestRegistry()
  const matches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  render(
    h(TestCommandHandler, {
      command: AppCommand.workbench.closeActiveTab,
      onMatch(match) {
        matches.push(match)
      },
    }),
    container,
  )

  try {
    await flushRenderEffects()
    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(false)

    dispatchKeydown(runtimeDocument, {
      key: "w",
      code: "KeyW",
      ctrlKey: true,
    })

    expect(matches).toEqual([])

    workbenchTabSet.openOrFocusTab({
      kind: "projects",
      props: {},
    })

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
    render(null, container)
    cleanup()
  }
})

test("closable tab drives runtime availability for closeActiveTab", async () => {
  const { registry, workbenchTabSet, cleanup } = createTestRegistry()
  const container = document.createElement("div")
  document.body.append(container)

  render(
    h(TestCommandHandler, {
      command: AppCommand.workbench.closeActiveTab,
      onMatch() {},
    }),
    container,
  )

  try {
    await flushRenderEffects()
    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(false)

    workbenchTabSet.openOrFocusTab({
      kind: "projects",
      props: {},
    })

    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(true)
  } finally {
    render(null, container)
    cleanup()
  }
})

test("active dialog layer lets launch-dialog selectors override the global project switcher binding", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const switchProjectMatches: unknown[] = []
  const projectMatches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  try {
    render(
      h(TestLayeredCommands, {
        dialogActive: false,
        onSwitchProject(match) {
          switchProjectMatches.push(match)
        },
        onProject(match) {
          projectMatches.push(match)
        },
        projectActive: true,
      }),
      container,
    )
    await flushRenderEffects()
    registry.applyKeymapSnapshot("goddard", {})

    dispatchKeydown(runtimeDocument, {
      key: "o",
      code: "KeyO",
      ctrlKey: true,
    })

    expect(switchProjectMatches).toHaveLength(1)
    expect(projectMatches).toHaveLength(0)

    render(
      h(TestLayeredCommands, {
        dialogActive: true,
        onSwitchProject(match) {
          switchProjectMatches.push(match)
        },
        onProject(match) {
          projectMatches.push(match)
        },
        projectActive: true,
      }),
      container,
    )
    await flushRenderEffects()

    expect(isCommandAvailable(registry.runtime, AppCommand.sessionInput.openProjectSelector)).toBe(
      true,
    )

    dispatchKeydown(runtimeDocument, {
      key: "o",
      code: "KeyO",
      ctrlKey: true,
    })

    expect(switchProjectMatches).toHaveLength(1)
    expect(projectMatches).toHaveLength(1)
  } finally {
    render(null, container)
    cleanup()
  }
})

test("session input commands require a handler in the active command layer", async () => {
  const { registry, cleanup } = createTestRegistry()
  const container = document.createElement("div")
  document.body.append(container)

  try {
    expect(isCommandAvailable(registry.runtime, AppCommand.sessionInput.openProjectSelector)).toBe(
      false,
    )

    render(
      h(TestCommandHandler, {
        command: AppCommand.sessionInput.openProjectSelector,
        onMatch() {},
      }),
      container,
    )
    await flushRenderEffects()

    expect(isCommandAvailable(registry.runtime, AppCommand.sessionInput.openProjectSelector)).toBe(
      true,
    )

    render(
      h(
        Fragment,
        {},
        h(TestCommandHandler, {
          command: AppCommand.sessionInput.openProjectSelector,
          onMatch() {},
        }),
        h(CommandLayerProvider, { active: true }, null),
      ),
      container,
    )
    await flushRenderEffects()

    expect(isCommandAvailable(registry.runtime, AppCommand.sessionInput.openProjectSelector)).toBe(
      false,
    )

    render(
      h(
        Fragment,
        {},
        h(TestCommandHandler, {
          command: AppCommand.sessionInput.openProjectSelector,
          onMatch() {},
        }),
        h(
          CommandLayerProvider,
          { active: true },
          h(TestCommandHandler, {
            command: AppCommand.sessionInput.openProjectSelector,
            onMatch() {},
          }),
        ),
      ),
      container,
    )
    await flushRenderEffects()

    expect(isCommandAvailable(registry.runtime, AppCommand.sessionInput.openProjectSelector)).toBe(
      true,
    )
  } finally {
    render(null, container)
    cleanup()
  }
})

test("command palette visibility can include handlers outside the active command layer", async () => {
  const { registry, cleanup } = createTestRegistry()
  const container = document.createElement("div")
  document.body.append(container)

  try {
    render(
      h(
        Fragment,
        {},
        h(TestCommandHandler, {
          command: AppCommand.navigation.openNewSessionDialog,
          onMatch() {},
        }),
        h(CommandLayerProvider, { active: true }, null),
      ),
      container,
    )
    await flushRenderEffects()

    expect(isCommandAvailable(registry.runtime, AppCommand.navigation.openNewSessionDialog)).toBe(
      false,
    )
    expect(
      isCommandPaletteVisible(registry.runtime, AppCommand.navigation.openNewSessionDialog),
    ).toBe(true)
  } finally {
    render(null, container)
    cleanup()
  }
})

test("inactive command layer bindings do not prevent default", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const matches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  try {
    render(
      h(
        Fragment,
        {},
        h(TestCommandHandler, {
          command: AppCommand.navigation.openCommandPalette,
          onMatch(match) {
            matches.push(match)
          },
        }),
        h(CommandLayerProvider, { active: true }, null),
      ),
      container,
    )
    await flushRenderEffects()
    registry.applyKeymapSnapshot("goddard", {})

    const event = dispatchKeydown(runtimeDocument, {
      key: "p",
      code: "KeyP",
      ctrlKey: true,
    })

    expect(matches).toEqual([])
    expect(event.defaultPrevented).toBe(false)
  } finally {
    render(null, container)
    cleanup()
  }
})

test("handler availability changes do not rebind shortcuts", async () => {
  const { registry, cleanup } = createTestRegistry()
  const container = document.createElement("div")
  document.body.append(container)

  try {
    registry.applyKeymapSnapshot("goddard", {})
    const bindingIds = registry.runtime.getBindings().map((binding) => binding.id)

    expect(isCommandAvailable(registry.runtime, AppCommand.navigation.openCommandPalette)).toBe(
      false,
    )

    render(
      h(TestCommandHandler, {
        command: AppCommand.navigation.openCommandPalette,
        onMatch() {},
      }),
      container,
    )
    await flushRenderEffects()

    expect(isCommandAvailable(registry.runtime, AppCommand.navigation.openCommandPalette)).toBe(
      true,
    )
    expect(registry.runtime.getBindings().map((binding) => binding.id)).toEqual(bindingIds)

    render(null, container)
    await flushRenderEffects()

    expect(isCommandAvailable(registry.runtime, AppCommand.navigation.openCommandPalette)).toBe(
      false,
    )
    expect(registry.runtime.getBindings().map((binding) => binding.id)).toEqual(bindingIds)
  } finally {
    render(null, container)
    cleanup()
  }
})
