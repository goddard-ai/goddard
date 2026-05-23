type WorkbenchCachedValue<TValue> = {
  dispose?: () => void
  setup?: () => void | (() => void)
  value: TValue
}

type WorkbenchTabCacheEntry<TValue = unknown> = WorkbenchCachedValue<TValue> & {
  setupCleanup: (() => void) | null
}

function buildEntryKey(tabId: string, key: string) {
  return `${tabId}\0${key}`
}

/** Retains explicitly cached detail-tab values beyond their rendering component lifetime. */
export class WorkbenchTabCache {
  // Runtime cache entries stay private because they are intentionally non-persisted app state.
  #entries = new Map<string, WorkbenchTabCacheEntry>()

  /** Returns one retained value, creating it the first time a tab asks for the key. */
  getOrCreate<TValue>(tabId: string, key: string, createValue: () => WorkbenchCachedValue<TValue>) {
    const entryKey = buildEntryKey(tabId, key)
    const existingEntry = this.#entries.get(entryKey)

    if (existingEntry) {
      return existingEntry.value as TValue
    }

    const nextValue = createValue()
    this.#entries.set(entryKey, {
      ...nextValue,
      setupCleanup: null,
    })

    return nextValue.value
  }

  /** Starts setup for one retained value after the requesting component commits. */
  setup(tabId: string, key: string) {
    const entry = this.#entries.get(buildEntryKey(tabId, key))

    if (!entry || entry.setupCleanup || !entry.setup) {
      return
    }

    entry.setupCleanup = entry.setup() ?? null
  }

  /** Disposes every retained value owned by one tab. */
  disposeTab(tabId: string) {
    for (const [entryKey, entry] of this.#entries) {
      if (!entryKey.startsWith(`${tabId}\0`)) {
        continue
      }

      this.#entries.delete(entryKey)
      entry.setupCleanup?.()
      entry.dispose?.()
    }
  }

  /** Disposes every retained value owned by the cache. */
  disposeAll() {
    const entryKeys = Array.from(this.#entries.keys())

    for (const entryKey of entryKeys) {
      const tabId = entryKey.slice(0, entryKey.indexOf("\0"))
      this.disposeTab(tabId)
    }
  }
}
