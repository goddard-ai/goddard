import { isPlainObject } from "radashi"

import { isAppearanceMode, type AppearanceMode } from "./theme.ts"

export const BOOT_APPEARANCE_STORAGE_KEY = "goddard:boot-appearance"

/** Minimal renderer-local appearance hint used before app-state hydration. */
export type BootAppearanceSnapshot = {
  mode: AppearanceMode
  highContrast: boolean
}

const DEFAULT_BOOT_APPEARANCE: BootAppearanceSnapshot = {
  mode: "system",
  highContrast: false,
}

function getBootAppearanceStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null
  } catch {
    return null
  }
}

export function parseBootAppearanceSnapshot(value: string | null): BootAppearanceSnapshot | null {
  if (!value) {
    return null
  }

  try {
    const parsed: unknown = JSON.parse(value)

    if (!isPlainObject(parsed)) {
      return null
    }

    const snapshot = parsed as Record<string, unknown>

    if (!isAppearanceMode(snapshot.mode) || typeof snapshot.highContrast !== "boolean") {
      return null
    }

    return {
      mode: snapshot.mode,
      highContrast: snapshot.highContrast,
    }
  } catch {
    return null
  }
}

export function readBootAppearanceSnapshot(
  storage: Storage | null = getBootAppearanceStorage(),
): BootAppearanceSnapshot {
  const snapshot = parseBootAppearanceSnapshot(
    storage?.getItem(BOOT_APPEARANCE_STORAGE_KEY) ?? null,
  )

  return snapshot ?? DEFAULT_BOOT_APPEARANCE
}

export function writeBootAppearanceSnapshot(
  snapshot: BootAppearanceSnapshot,
  storage: Storage | null = getBootAppearanceStorage(),
): void {
  try {
    storage?.setItem(BOOT_APPEARANCE_STORAGE_KEY, JSON.stringify(snapshot))
  } catch {
    // Private browsing and storage quotas must not block first-paint theming.
  }
}
