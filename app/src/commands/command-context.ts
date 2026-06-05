import { computed, effect } from "@preact/signals"
import { createShortcuts, type RunnableInput, type ShortcutRuntime } from "powerkeys"
import { Sigma } from "preact-sigma"

import { isAppCommandHandled, isAppCommandHandledAnywhere } from "~/commands/app-command.ts"
import type { MainTab } from "~/main-tab.ts"
import type { AppCommandId } from "~/shared/app-commands.ts"
import { WORKBENCH_MAIN_TAB, type WorkbenchTabSet } from "~/workbench-tab-set.ts"

/** Public state for command dispatch context that is not derived from app navigation state. */
export type CommandContextState = {
  activeScopes: readonly string[]
}

/** App-scoped command context and Powerkeys runtime owner. */
export class CommandContext extends Sigma<CommandContextState> {
  /** Imperative powerkeys runtime that owns document listeners outside persisted shortcut state. */
  #runtime: ShortcutRuntime
  #mainTab: MainTab
  #workbenchTabSet: WorkbenchTabSet

  readonly #activeTabKind = computed(() => {
    return this.#workbenchTabSet.activeClosableTab?.kind ?? this.#mainTab.selectedKind
  })
  readonly #hasClosableActiveTab = computed(() => {
    return this.#workbenchTabSet.activeTabId !== WORKBENCH_MAIN_TAB.id
  })
  readonly whenContext = computed(() => {
    return {
      "workbench.activeTabKind": this.#activeTabKind.value,
      "workbench.hasClosableActiveTab": this.#hasClosableActiveTab.value,
      "mainTab.selectedKind": this.#mainTab.selectedKind,
    }
  })

  constructor(input: {
    mainTab: MainTab
    workbenchTabSet: WorkbenchTabSet
    target?: Document | HTMLElement
  }) {
    super({
      activeScopes: [],
    })

    this.#mainTab = input.mainTab
    this.#workbenchTabSet = input.workbenchTabSet
    this.#runtime = createShortcuts({
      target: input.target ?? document,
      editablePolicy: "allow-if-meta",
      getActiveScopes: () => this.activeScopes,
      canDispatch: (candidate) => {
        const commandId =
          "id" in candidate.handler && typeof candidate.handler.id === "string"
            ? (candidate.handler.id as AppCommandId)
            : null

        return commandId === null || isAppCommandHandled(commandId)
      },
      onError: (error, info) => {
        console.error("Shortcut runtime error.", error, info)
      },
    })
  }

  get runtime() {
    return this.#runtime
  }

  /** Replaces the active shortcut scopes consulted by the Powerkeys runtime. */
  setActiveScopes(scopes: readonly string[]) {
    this.activeScopes = scopes
  }

  onSetup() {
    return [
      this.#runtime,
      effect(() => {
        this.#runtime.batchContext(this.whenContext.value)
      }),
    ]
  }
}

export interface CommandContext extends CommandContextState {}

/** Reads app command handler state before delegating to the runtime availability check. */
export function isCommandAvailable(runtime: ShortcutRuntime, input: RunnableInput) {
  const commandId = "id" in input ? (input.id as AppCommandId) : null

  if (commandId && !isAppCommandHandled(commandId)) {
    return false
  }

  return runtime.isAvailable(input)
}

/** Checks whether a command can be shown from the command palette across command layers. */
export function isCommandPaletteVisible(runtime: ShortcutRuntime, input: RunnableInput) {
  const commandId = "id" in input ? (input.id as AppCommandId) : null

  if (commandId && !isAppCommandHandledAnywhere(commandId)) {
    return false
  }

  return runtime.isAvailable(input)
}
