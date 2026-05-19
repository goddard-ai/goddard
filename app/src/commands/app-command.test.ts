import { expect, test } from "bun:test"
import { Fragment, h, render } from "preact"

import { ShortcutRegistry } from "~/shortcuts/shortcut-registry.ts"
import { AppCommand, useAppCommand } from "./app-command.ts"
import { commandContext, isCommandAvailable } from "./command-context.ts"
import { CommandLayerProvider } from "./command-layer.tsrx"

/** Creates one registry instance with an isolated document-like event boundary. */
function createTestRegistry() {
  const runtimeDocument = document.implementation.createHTMLDocument("app-command-test")
  const registry = new ShortcutRegistry(runtimeDocument)
  const cleanup = registry.setup()
  commandContext.activeScopes.value = []
  commandContext.activeTabKind.value = "inbox"
  commandContext.hasClosableActiveTab.value = false
  commandContext.selectedKind.value = "inbox"

  return {
    registry,
    runtimeDocument,
    cleanup,
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
  onPalette: (match?: unknown) => void
  onProject: (match?: unknown) => void
  projectActive: boolean
}) {
  return h(
    Fragment,
    {},
    h(TestCommandHandler, {
      command: AppCommand.navigation.openCommandPalette,
      onMatch: props.onPalette,
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
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
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
    render(null, container)
    cleanup()
  }
})

test("closable tab drives runtime availability for closeActiveTab", async () => {
  const { registry, cleanup } = createTestRegistry()
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

    commandContext.hasClosableActiveTab.value = true

    expect(isCommandAvailable(registry.runtime, AppCommand.workbench.closeActiveTab)).toBe(true)
  } finally {
    render(null, container)
    cleanup()
  }
})

test("active dialog layer lets launch-dialog selectors override the global palette binding", async () => {
  const { registry, runtimeDocument, cleanup } = createTestRegistry()
  const paletteMatches: unknown[] = []
  const projectMatches: unknown[] = []
  const container = runtimeDocument.createElement("div")
  runtimeDocument.body.append(container)

  try {
    render(
      h(TestLayeredCommands, {
        dialogActive: false,
        onPalette(match) {
          paletteMatches.push(match)
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
      key: "p",
      code: "KeyP",
      ctrlKey: true,
    })

    expect(paletteMatches).toHaveLength(1)
    expect(projectMatches).toHaveLength(0)

    render(
      h(TestLayeredCommands, {
        dialogActive: true,
        onPalette(match) {
          paletteMatches.push(match)
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
      key: "p",
      code: "KeyP",
      ctrlKey: true,
    })

    expect(paletteMatches).toHaveLength(1)
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
