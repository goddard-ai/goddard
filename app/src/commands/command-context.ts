import { computed, signal } from "@preact/signals"
import type { RunnableInput, ShortcutRuntime } from "powerkeys"

import { isAppCommandHandled } from "~/commands/app-command.ts"
import type { MainTabItemId } from "~/main-tab-items.ts"
import type { AppCommandId } from "~/shared/app-commands.ts"
import type { WorkbenchContentKind } from "~/workbench-tab-set.ts"

const activeScopes = signal<readonly string[]>([])
const activeTabKind = signal<WorkbenchContentKind>("inbox")
const hasClosableActiveTab = signal(false)
const selectedKind = signal<MainTabItemId>("inbox")

const whenContext = computed(() => {
  return {
    "workbench.activeTabKind": activeTabKind.value,
    "workbench.hasClosableActiveTab": hasClosableActiveTab.value,
    "mainTab.selectedKind": selectedKind.value,
  }
})

export const commandContext = {
  activeScopes,
  activeTabKind,
  hasClosableActiveTab,
  selectedKind,
  whenContext,
} as const

const commandAvailabilitySnapshot = computed(() => ({
  activeScopes: activeScopes.value,
  whenContext: whenContext.value,
}))

/** Reads the reactive command-context inputs before delegating to the runtime. */
export function isCommandAvailable(runtime: ShortcutRuntime, input: RunnableInput) {
  const snapshot = commandAvailabilitySnapshot.value
  runtime.batchContext(snapshot.whenContext)

  const commandId = "id" in input ? (input.id as AppCommandId) : null

  if (commandId && !isAppCommandHandled(commandId)) {
    return false
  }

  return runtime.isAvailable(input)
}
