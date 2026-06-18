export type WatchEventSource = "worktree" | "git"

export type WatchEventDetail = {
  source: WatchEventSource
  path: string
  recursive: boolean
  eventType: string
  filename: string | null
}

/** Avoids routing duplicate Git metadata events from the main worktree content watcher. */
export function shouldIgnoreWatchEvent(source: WatchEventSource, filename: string | Buffer | null) {
  if (source !== "worktree" || filename === null) {
    return false
  }

  const path = filename.toString()
  return path === ".git" || path.startsWith(".git/") || path.startsWith(".git\\")
}

/** Creates a small event queue that can also wake on aborts or watcher errors. */
export function createWatchEventQueue(signal: AbortSignal | undefined) {
  let pending = false
  const pendingEvents: WatchEventDetail[] = []
  let failure: unknown
  const waiters = new Set<(value: boolean) => void>()

  const flushWaiters = (value: boolean) => {
    for (const waiter of waiters) {
      waiter(value)
    }
    waiters.clear()
  }

  const waitForEvent = () => waitForEventOrTimeout(null)
  const waitForEventOrTimeout = (timeoutMs: number | null) => {
    if (failure) {
      throw failure
    }
    if (pending) {
      pending = false
      return Promise.resolve(true)
    }
    if (isAbortSignalAborted(signal)) {
      return Promise.resolve(false)
    }

    return new Promise<boolean>((resolvePromise, rejectPromise) => {
      let timeout: ReturnType<typeof setTimeout> | null = null
      const done = (value: boolean) => {
        if (timeout) {
          clearTimeout(timeout)
        }
        waiters.delete(done)
        signal?.removeEventListener("abort", abort)

        if (failure) {
          rejectPromise(failure)
          return
        }
        resolvePromise(value)
      }
      const abort = () => done(false)

      waiters.add(done)
      signal?.addEventListener("abort", abort, { once: true })

      if (timeoutMs !== null) {
        timeout = setTimeout(() => done(false), timeoutMs)
      }
    })
  }

  return {
    notify: (event?: WatchEventDetail) => {
      pending = true
      if (event) {
        pendingEvents.push(event)
      }
      flushWaiters(true)
    },
    drainEvents: () => pendingEvents.splice(0),
    fail: (error: unknown) => {
      failure = error
      flushWaiters(false)
    },
    waitForEvent,
    waitForEventOrTimeout,
  }
}

export type WatchEventQueue = ReturnType<typeof createWatchEventQueue>

/** Waits until filesystem events have been quiet long enough for Git to settle. */
export async function waitForWatchQuietPeriod(
  events: WatchEventQueue,
  signal: AbortSignal | undefined,
  watchDebounceMs: number,
) {
  while (!isAbortSignalAborted(signal)) {
    const changed = await events.waitForEventOrTimeout(watchDebounceMs)
    if (!changed) {
      return !isAbortSignalAborted(signal)
    }
  }

  return false
}

/** Checks an abort signal without causing TypeScript to over-narrow loop state. */
export function isAbortSignalAborted(signal: AbortSignal | undefined) {
  return signal?.aborted === true
}
