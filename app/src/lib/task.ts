import { computed, signal } from "@preact/signals"

type TaskStatus = "idle" | "pending" | "failed"

/** Creates signal-backed lifecycle state for one UI-triggered async operation. */
export function createTask<TaskError = unknown>() {
  const status = signal<TaskStatus>("idle")
  const error = signal<TaskError | null>(null)
  const isPending = computed(() => status.value === "pending")

  function clearError() {
    error.value = null

    if (status.value === "failed") {
      status.value = "idle"
    }
  }

  async function run(operation: () => Promise<unknown>) {
    if (isPending.value) {
      return
    }

    status.value = "pending"
    error.value = null

    try {
      await operation()
      status.value = "idle"
      error.value = null
    } catch (cause) {
      error.value = cause as TaskError
      status.value = "failed"

      throw cause
    }
  }

  return {
    clearError,
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

  async function run(key: TaskKey, operation: () => Promise<unknown>) {
    if (isPending.value) {
      return
    }

    status.value = "pending"
    activeKey.value = key
    error.value = null

    try {
      await operation()
      status.value = "idle"
      activeKey.value = null
      error.value = null
    } catch (cause) {
      status.value = "failed"
      activeKey.value = null
      error.value = cause as TaskError

      throw cause
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
