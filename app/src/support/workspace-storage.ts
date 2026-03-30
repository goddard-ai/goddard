/**
 * Reads one JSON payload from local storage and falls back cleanly when parsing fails.
 */
export function readJsonStorage<T>(key: string, fallback: T): T {
  if (typeof window === "undefined") {
    return fallback
  }

  try {
    const rawValue = window.localStorage.getItem(key)
    return rawValue ? (JSON.parse(rawValue) as T) : fallback
  } catch {
    return fallback
  }
}

/**
 * Writes one JSON payload into local storage when the browser runtime is available.
 */
export function writeJsonStorage(key: string, value: unknown): void {
  if (typeof window === "undefined") {
    return
  }

  window.localStorage.setItem(key, JSON.stringify(value))
}
