import { Sigma } from "preact-sigma"

const RECENT_USED_VALUE_LIMIT = 10

/** Persisted app-owned selector defaults and most-recently-used value order. */
export type SelectorUsageState = {
  currentValuesByKey: Record<string, string | null>
  recentUsedValuesByKey: Record<string, string[]>
}

/** Stable keys for app selector usage records. */
export const SelectorUsageKey = {
  projectSwitcher: "project.switcher",
  sessionControlMode(agentId: string) {
    return `session.control.mode:${agentId}`
  },
  sessionControlModel(agentId: string) {
    return `session.control.model:${agentId}`
  },
  sessionControlThinking(agentId: string) {
    return `session.control.thinking:${agentId}`
  },
  sessionLaunchAgent: "session.launch.agent",
  sessionLaunchBranch(repoRoot: string) {
    return `session.launch.branch:${repoRoot}`
  },
  sessionLaunchCwd(projectPath: string) {
    return `session.launch.cwd:${projectPath}`
  },
  sessionLaunchLocation: "session.launch.location",
} as const

/** Public selector usage operations used by app components and launch preference helpers. */
export type SelectorUsageStore = {
  getCurrentValue(key: string): string | null
  getRecentUsedValues(key: string): readonly string[]
  recordUsedValue(key: string, value: string | null | undefined): void
  setCurrentValue(key: string, value: string | null | undefined): void
}

/** Orders selector items by selected value, recent use, then natural item order. */
export function orderSelectorItemsByUsage<const TItem>(
  items: readonly TItem[],
  input: {
    getValue?: (item: TItem) => string
    recentUsedValues?: readonly string[] | null
    selectedValue?: string | null
  },
) {
  const getValue = input.getValue ?? ((item) => (item as { value: string }).value)
  const itemByValue = new Map(items.map((item) => [getValue(item), item]))
  const orderedItems: TItem[] = []
  const usedValues = new Set<string>()

  function appendValue(value: string | null | undefined) {
    if (!value || usedValues.has(value)) {
      return
    }

    const item = itemByValue.get(value)

    if (!item) {
      return
    }

    orderedItems.push(item)
    usedValues.add(value)
  }

  appendValue(input.selectedValue)

  for (const value of input.recentUsedValues ?? []) {
    appendValue(value)
  }

  for (const item of items) {
    appendValue(getValue(item))
  }

  return orderedItems
}

/** App-wide owner for durable selector current values and most-recently-used order. */
export class SelectorUsage extends Sigma<SelectorUsageState> {
  constructor(
    input: SelectorUsageState = {
      currentValuesByKey: {},
      recentUsedValuesByKey: {},
    },
  ) {
    super(input)
  }

  /** Returns the current selected value remembered for one selector key. */
  getCurrentValue(key: string) {
    return this.currentValuesByKey[key] ?? null
  }

  /** Returns available recent-used values for one selector key in newest-first order. */
  getRecentUsedValues(key: string) {
    return [...(this.recentUsedValuesByKey[key] ?? [])]
  }

  /** Remembers a selector's current selected value without changing its MRU order. */
  setCurrentValue(key: string, value: string | null | undefined) {
    const nextValue = value ?? null

    if (this.currentValuesByKey[key] === nextValue) {
      return
    }

    this.currentValuesByKey = {
      ...this.currentValuesByKey,
      [key]: nextValue,
    }
  }

  /** Records one successfully used selector value and moves it to the front of MRU order. */
  recordUsedValue(key: string, value: string | null | undefined) {
    if (!value) {
      return
    }

    const currentValues = this.recentUsedValuesByKey[key] ?? []
    const nextValues = [value, ...currentValues.filter((item) => item !== value)].slice(
      0,
      RECENT_USED_VALUE_LIMIT,
    )

    this.currentValuesByKey = {
      ...this.currentValuesByKey,
      [key]: value,
    }
    this.recentUsedValuesByKey = {
      ...this.recentUsedValuesByKey,
      [key]: nextValues,
    }
  }
}

export interface SelectorUsage extends SelectorUsageState {}
