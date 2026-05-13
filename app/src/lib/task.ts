import { computed, signal } from "@preact/signals"

/** Creates signal-backed lifecycle state for one UI-triggered async operation. */
export function createTask() {
  const status = signal<"idle" | "pending" | "failed">("idle")
  const error = signal<unknown>(null)
  const isPending = computed(() => status.value === "pending")
  let activePromise: Promise<unknown> | null = null

  async function run<Result>(operation: () => Promise<Result>) {
    if (activePromise) {
      return activePromise as Promise<Result>
    }

    status.value = "pending"
    error.value = null

    const promise = Promise.resolve().then(operation)
    activePromise = promise

    try {
      const result = await promise

      if (activePromise === promise) {
        status.value = "idle"
        error.value = null
      }

      return result
    } catch (cause) {
      if (activePromise === promise) {
        error.value = cause
        status.value = "failed"
      }

      throw cause
    } finally {
      if (activePromise === promise) {
        activePromise = null
      }
    }
  }

  return {
    error,
    isPending,
    run,
    status,
  }
}
