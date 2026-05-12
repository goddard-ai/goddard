import { computed, signal } from "@preact/signals"
import type { RunnableInput, ShortcutRuntime } from "powerkeys"

import { isAppCommandHandled } from "~/commands/app-command.ts"
import { hasOpenModalDialog } from "~/lib/modal-stack.ts"
import type { NavigationItemId } from "~/navigation-items.ts"
import type { AppCommandId } from "~/shared/app-commands.ts"
import type { WorkbenchTabKind } from "~/workbench-tab-set.ts"

const activeScopes = signal<readonly string[]>([])
const activeTabKind = signal<WorkbenchTabKind>("main")
const hasClosableActiveTab = signal(false)
const selectedNavId = signal<NavigationItemId>("inbox")
const hasCloseTarget = computed(() => {
  return hasClosableActiveTab.value || hasOpenModalDialog.value
})

const whenContext = computed(() => {
  return {
    "workbench.activeTabKind": activeTabKind.value,
    "workbench.hasCloseTarget": hasCloseTarget.value,
    "workbench.hasClosableActiveTab": hasClosableActiveTab.value,
    "workbench.hasOpenModal": hasOpenModalDialog.value,
    "navigation.selectedNavId": selectedNavId.value,
  }
})

export const commandContext = {
  activeScopes,
  activeTabKind,
  hasCloseTarget,
  hasClosableActiveTab,
  hasOpenModalDialog,
  selectedNavId,
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
