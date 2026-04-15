import { computed, signal } from "@preact/signals"
import type { RunnableInput, ShortcutRuntime } from "powerkeys"

import type { NavigationItemId } from "~/navigation.ts"
import type { WorkbenchTabKind } from "~/workbench-tab-set.ts"

const activeScopes = signal<readonly string[]>([])
const activeTabKind = signal<WorkbenchTabKind>("main")
const hasClosableActiveTab = signal(false)
const selectedNavId = signal<NavigationItemId>("inbox")
const sessionInputActive = signal(false)
const sessionInputCanSubmit = signal(false)
const sessionInputHasModelSelector = signal(false)
const sessionInputHasProjectSelector = signal(false)
const sessionInputHasThinkingLevel = signal(false)

const whenContext = computed(() => {
  return {
    "sessionInput.isActive": sessionInputActive.value,
    "sessionInput.canSubmit": sessionInputCanSubmit.value,
    "sessionInput.hasModelSelector": sessionInputHasModelSelector.value,
    "sessionInput.hasProjectSelector": sessionInputHasProjectSelector.value,
    "sessionInput.hasThinkingLevel": sessionInputHasThinkingLevel.value,
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
  sessionInputActive,
  sessionInputCanSubmit,
  sessionInputHasModelSelector,
  sessionInputHasProjectSelector,
  sessionInputHasThinkingLevel,
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
