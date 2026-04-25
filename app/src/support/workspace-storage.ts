import type { PersistRecord, SyncPersistStore } from "preact-sigma/persist"

function isPersistRecord<TStored>(value: unknown): value is PersistRecord<TStored> {
  return Boolean(
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    typeof (value as { version?: unknown }).version === "number" &&
    typeof (value as { savedAt?: unknown }).savedAt === "number" &&
    "value" in value,
  )
}

/** Creates one localStorage-backed JSON store for `preact-sigma/persist` records. */
export function createWorkspaceStorageStore<TStored>(): SyncPersistStore<TStored> {
  return {
    get(key) {
      if (typeof window === "undefined") {
        return undefined
      }

      try {
        const rawValue = window.localStorage.getItem(key)
        const parsed = rawValue ? (JSON.parse(rawValue) as unknown) : undefined

        return isPersistRecord<TStored>(parsed) ? parsed : undefined
      } catch {
        return undefined
      }
    },

    set(key, record) {
      if (typeof window === "undefined") {
        return
      }

      window.localStorage.setItem(key, JSON.stringify(record))
    },

    delete(key) {
      if (typeof window === "undefined") {
        return
      }

      window.localStorage.removeItem(key)
    },
  }
}
