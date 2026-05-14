import { computed, signal } from "@preact/signals"

type TaskStatus = "idle" | "pending" | "failed"

/** Creates signal-backed lifecycle state for one UI-triggered async operation. */
export function createTask<TaskError = unknown>() {
  const status = signal<TaskStatus>("idle")
  const error = signal<TaskError | null>(null)
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
        error.value = cause as TaskError
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

/** Creates signal-backed lifecycle state for a mutually exclusive keyed action group. */
export function createKeyedTask<TaskKey, TaskError = unknown>() {
  const status = signal<TaskStatus>("idle")
  const activeKey = signal<TaskKey | null>(null)
  const error = signal<TaskError | null>(null)
  const isPending = computed(() => status.value === "pending")
  let activePromise: Promise<unknown> | null = null

  async function run<Result>(key: TaskKey, operation: () => Promise<Result>) {
    if (activePromise) {
      return activePromise as Promise<Result>
    }

    status.value = "pending"
    activeKey.value = key
    error.value = null

    const promise = Promise.resolve().then(operation)
    activePromise = promise

    try {
      const result = await promise

      if (activePromise === promise) {
        status.value = "idle"
        activeKey.value = null
        error.value = null
      }

      return result
    } catch (cause) {
      if (activePromise === promise) {
        status.value = "failed"
        activeKey.value = null
        error.value = cause as TaskError
      }

      throw cause
    } finally {
      if (activePromise === promise) {
        activePromise = null
      }
    }
  }

  return {
    activeKey,
    error,
    isPending,
    run,
    status,
  }
}
