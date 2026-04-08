import { expect, test, vi } from "vitest"
import { QueryClient } from "./query.ts"

async function waitForSuspendedRead<T>(read: () => T) {
  try {
    read()
  } catch (value) {
    if (value instanceof Promise) {
      await value
      return
    }

    throw value
  }

  throw new Error("Expected query read to suspend.")
}

function createDeferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve
  })

  return {
    promise,
    resolve,
  }
}

test("QueryClient.read suspends until the first result is cached", async () => {
  const queryClient = new QueryClient()
  const loadSessionCount = vi.fn(async (projectPath: string) => projectPath.length)

  await waitForSuspendedRead(() => queryClient.read(loadSessionCount, ["/repo-a"]))

  expect(queryClient.read(loadSessionCount, ["/repo-a"])).toBe(7)
})

test("QueryClient.invalidate keeps stale data visible until the refetch crosses the suspense delay", async () => {
  vi.useFakeTimers()

  try {
    let deferred = createDeferred<string>()
    const notifications: string[] = []
    const queryClient = new QueryClient()
    const loadSession = vi.fn((_sessionId: string) => deferred.promise)

    queryClient.subscribe(loadSession, ["ses_1"], () => {
      notifications.push("update")
    })

    const firstLoad = waitForSuspendedRead(() => queryClient.read(loadSession, ["ses_1"]))
    await Promise.resolve()
    deferred.resolve("first")
    await firstLoad

    expect(queryClient.read(loadSession, ["ses_1"])).toBe("first")
    expect(notifications).toEqual(["update"])

    deferred = createDeferred<string>()
    loadSession.mockReturnValueOnce(deferred.promise)
    queryClient.invalidate(loadSession, ["ses_1"])
    await Promise.resolve()

    expect(queryClient.read(loadSession, ["ses_1"])).toBe("first")

    await vi.advanceTimersByTimeAsync(1_249)
    expect(notifications).toEqual(["update"])
    expect(queryClient.read(loadSession, ["ses_1"])).toBe("first")

    await vi.advanceTimersByTimeAsync(1)
    expect(notifications).toEqual(["update", "update"])

    const slowRefetch = waitForSuspendedRead(() => queryClient.read(loadSession, ["ses_1"]))
    await Promise.resolve()
    deferred.resolve("second")
    await slowRefetch

    expect(queryClient.read(loadSession, ["ses_1"])).toBe("second")
    expect(notifications).toEqual(["update", "update", "update"])
  } finally {
    vi.useRealTimers()
  }
})
