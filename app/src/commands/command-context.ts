import { computed, signal } from "@preact/signals"
import type { RunnableInput, ShortcutRuntime } from "powerkeys"

import type { NavigationItemId } from "~/navigation.ts"
import type { WorkbenchTabKind } from "~/workbench-tab-set.ts"

const activeScopes = signal<readonly string[]>([])
const activeTabKind = signal<WorkbenchTabKind>("main")
const hasClosableActiveTab = signal(false)
const selectedNavId = signal<NavigationItemId>("inbox")

const whenContext = computed(() => {
  return {
    "workbench.activeTabKind": activeTabKind.value,
    "workbench.hasClosableActiveTab": hasClosableActiveTab.value,
    "navigation.selectedNavId": selectedNavId.value,
  }
})

export const commandContext = {
  activeScopes,
  activeTabKind,
  hasClosableActiveTab,
  selectedNavId,
  whenContext,
} as const

const commandAvailabilitySnapshot = computed(() => ({
  activeScopes: activeScopes.value,
  whenContext: whenContext.value,
}))

/** Reads the reactive command-context inputs before delegating to the runtime. */
export function isCommandAvailable(runtime: ShortcutRuntime, input: RunnableInput) {
  void commandAvailabilitySnapshot.value
  return runtime.isAvailable(input)
}
