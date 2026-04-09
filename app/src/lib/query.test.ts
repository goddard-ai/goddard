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
  const queryKey = queryClient.getQueryKey(loadSessionCount, ["/repo-a"])

  await waitForSuspendedRead(() => queryClient.read(queryKey, loadSessionCount, ["/repo-a"]))

  expect(queryClient.read(queryKey, loadSessionCount, ["/repo-a"])).toBe(7)
})

test("QueryClient.invalidate keeps stale data visible until the refetch resolves", async () => {
  let deferred = createDeferred<string>()
  const notifications: string[] = []
  const refetchSettled = createDeferred<void>()
  const queryClient = new QueryClient()
  const loadSession = vi.fn((_sessionId: string) => deferred.promise)
  const queryKey = queryClient.getQueryKey(loadSession, ["ses_1"])

  const firstLoad = waitForSuspendedRead(() => queryClient.read(queryKey, loadSession, ["ses_1"]))
  await Promise.resolve()
  deferred.resolve("first")
  await firstLoad

  expect(queryClient.read(queryKey, loadSession, ["ses_1"])).toBe("first")

  queryClient.subscribe(queryKey, () => {
    notifications.push("update")

    if (notifications.length === 1) {
      refetchSettled.resolve()
    }
  })

  deferred = createDeferred<string>()
  loadSession.mockReturnValueOnce(deferred.promise)
  queryClient.invalidate(loadSession, ["ses_1"])
  await Promise.resolve()

  expect(queryClient.read(queryKey, loadSession, ["ses_1"])).toBe("first")
  expect(notifications).toEqual([])

  deferred.resolve("second")
  await refetchSettled.promise

  expect(queryClient.read(queryKey, loadSession, ["ses_1"])).toBe("second")
  expect(notifications).toEqual(["update"])
})

test("QueryClient.subscribe refetches cached data when a query becomes active again", async () => {
  let deferred = createDeferred<string>()
  const notifications: string[] = []
  const refetchSettled = createDeferred<void>()
  const queryClient = new QueryClient()
  const loadSession = vi.fn((_sessionId: string) => deferred.promise)
  const queryKey = queryClient.getQueryKey(loadSession, ["ses_1"])

  const firstLoad = waitForSuspendedRead(() => queryClient.read(queryKey, loadSession, ["ses_1"]))
  await Promise.resolve()
  deferred.resolve("first")
  await firstLoad

  deferred = createDeferred<string>()
  loadSession.mockReturnValueOnce(deferred.promise)

  queryClient.subscribe(queryKey, () => {
    notifications.push("update")

    if (notifications.length === 1) {
      refetchSettled.resolve()
    }
  })

  await Promise.resolve()

  expect(queryClient.read(queryKey, loadSession, ["ses_1"])).toBe("first")
  expect(loadSession).toHaveBeenCalledTimes(2)
  expect(notifications).toEqual([])

  deferred.resolve("second")
  await refetchSettled.promise

  expect(queryClient.read(queryKey, loadSession, ["ses_1"])).toBe("second")
  expect(notifications).toEqual(["update"])
})

test("QueryClient.refetchActiveQueries refreshes only subscribed queries", async () => {
  let activeDeferred = createDeferred<string>()
  let inactiveDeferred = createDeferred<string>()
  const activeQueryNotifications: string[] = []
  const activeRefetchSettled = createDeferred<void>()
  let isWaitingForActiveRefetch = false
  const queryClient = new QueryClient()
  const loadActiveSession = vi.fn((_sessionId: string) => activeDeferred.promise)
  const loadInactiveSession = vi.fn((_sessionId: string) => inactiveDeferred.promise)
  const activeQueryKey = queryClient.getQueryKey(loadActiveSession, ["ses_active"])
  const inactiveQueryKey = queryClient.getQueryKey(loadInactiveSession, ["ses_inactive"])

  const firstActiveLoad = waitForSuspendedRead(() =>
    queryClient.read(activeQueryKey, loadActiveSession, ["ses_active"]),
  )
  const firstInactiveLoad = waitForSuspendedRead(() =>
    queryClient.read(inactiveQueryKey, loadInactiveSession, ["ses_inactive"]),
  )

  queryClient.subscribe(activeQueryKey, () => {
    activeQueryNotifications.push("update")

    if (isWaitingForActiveRefetch && activeQueryNotifications.length === 2) {
      activeRefetchSettled.resolve()
    }
  })

  await Promise.resolve()
  activeDeferred.resolve("active:first")
  inactiveDeferred.resolve("inactive:first")
  await firstActiveLoad
  await firstInactiveLoad

  activeDeferred = createDeferred<string>()
  loadActiveSession.mockReturnValueOnce(activeDeferred.promise)
  isWaitingForActiveRefetch = true

  queryClient.refetchActiveQueries()
  await Promise.resolve()

  expect(loadActiveSession).toHaveBeenCalledTimes(2)
  expect(loadInactiveSession).toHaveBeenCalledTimes(1)
  expect(queryClient.read(activeQueryKey, loadActiveSession, ["ses_active"])).toBe("active:first")
  expect(queryClient.read(inactiveQueryKey, loadInactiveSession, ["ses_inactive"])).toBe(
    "inactive:first",
  )

  activeDeferred.resolve("active:second")
  await activeRefetchSettled.promise

  expect(queryClient.read(activeQueryKey, loadActiveSession, ["ses_active"])).toBe("active:second")
  expect(activeQueryNotifications).toEqual(["update", "update"])
})
