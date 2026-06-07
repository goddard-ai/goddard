import { expect, test, vi } from "bun:test"

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

test("QueryClient.write updates one resolved cache entry without creating missing entries", async () => {
  let deferred = createDeferred<number>()
  const notifications: string[] = []
  const queryClient = new QueryClient()
  const loadSessionCount = vi.fn((_sessionId: string) => deferred.promise)
  const queryKey = queryClient.getQueryKey(loadSessionCount, ["ses_1"])

  const firstLoad = waitForSuspendedRead(() =>
    queryClient.read(queryKey, loadSessionCount, ["ses_1"]),
  )
  await Promise.resolve()
  deferred.resolve(1)
  await firstLoad

  deferred = createDeferred<number>()
  loadSessionCount.mockReturnValueOnce(deferred.promise)
  queryClient.subscribe(queryKey, () => {
    notifications.push("update")
  })

  expect(queryClient.write(loadSessionCount, ["ses_missing"], (count) => count + 1)).toBe(false)
  expect(queryClient.write(loadSessionCount, ["ses_1"], (count) => count + 1)).toBe(true)
  expect(queryClient.read(queryKey, loadSessionCount, ["ses_1"])).toBe(2)
  expect(notifications).toEqual(["update"])
})

test("QueryClient.writeAll updates every resolved cache entry for one query function", async () => {
  const notifications: string[] = []
  const queryClient = new QueryClient()
  const loadSessionCount = vi.fn(
    async (sessionId: string): Promise<number> => (sessionId === "ses_1" ? 1 : 10),
  )
  const loadProjectCount = vi.fn(async (_projectPath: string): Promise<number> => 100)
  const firstQueryKey = queryClient.getQueryKey(loadSessionCount, ["ses_1"])
  const secondQueryKey = queryClient.getQueryKey(loadSessionCount, ["ses_2"])
  const otherQueryKey = queryClient.getQueryKey(loadProjectCount, ["/repo"])

  await waitForSuspendedRead(() => queryClient.read(firstQueryKey, loadSessionCount, ["ses_1"]))
  await waitForSuspendedRead(() => queryClient.read(secondQueryKey, loadSessionCount, ["ses_2"]))
  await waitForSuspendedRead(() => queryClient.read(otherQueryKey, loadProjectCount, ["/repo"]))

  loadSessionCount.mockImplementation(() => new Promise(() => {}))
  loadProjectCount.mockImplementation(() => new Promise(() => {}))
  queryClient.subscribe(firstQueryKey, () => {
    notifications.push("first")
  })
  queryClient.subscribe(secondQueryKey, () => {
    notifications.push("second")
  })
  queryClient.subscribe(otherQueryKey, () => {
    notifications.push("other")
  })

  expect(queryClient.writeAll(loadSessionCount, (count) => count + 1)).toBe(2)

  expect(queryClient.read(firstQueryKey, loadSessionCount, ["ses_1"])).toBe(2)
  expect(queryClient.read(secondQueryKey, loadSessionCount, ["ses_2"])).toBe(11)
  expect(queryClient.read(otherQueryKey, loadProjectCount, ["/repo"])).toBe(100)
  expect(notifications).toEqual(["first", "second"])
})

test("QueryClient.writeAll does not notify when updater returns existing data", async () => {
  const notifications: string[] = []
  const queryClient = new QueryClient()
  const loadSession = vi.fn(
    async (_sessionId: string): Promise<{ id: string }> => ({
      id: "ses_1",
    }),
  )
  const queryKey = queryClient.getQueryKey(loadSession, ["ses_1"])

  await waitForSuspendedRead(() => queryClient.read(queryKey, loadSession, ["ses_1"]))

  loadSession.mockImplementation(() => new Promise(() => {}))
  queryClient.subscribe(queryKey, () => {
    notifications.push("update")
  })
  const currentSession = queryClient.read(queryKey, loadSession, ["ses_1"])

  expect(queryClient.writeAll(loadSession, (session) => session)).toBe(0)
  expect(queryClient.read(queryKey, loadSession, ["ses_1"])).toBe(currentSession)
  expect(notifications).toEqual([])
})

test("QueryClient.evict drops inactive cached data before the next read", async () => {
  let deferred = createDeferred<string>()
  const queryClient = new QueryClient()
  const loadSession = vi.fn((_sessionId: string) => deferred.promise)
  const queryKey = queryClient.getQueryKey(loadSession, ["ses_1"])

  const firstLoad = waitForSuspendedRead(() => queryClient.read(queryKey, loadSession, ["ses_1"]))
  await Promise.resolve()
  deferred.resolve("first")
  await firstLoad

  expect(queryClient.read(queryKey, loadSession, ["ses_1"])).toBe("first")

  queryClient.evict(loadSession, ["ses_1"])
  deferred = createDeferred<string>()
  loadSession.mockReturnValueOnce(deferred.promise)

  const secondLoad = waitForSuspendedRead(() => queryClient.read(queryKey, loadSession, ["ses_1"]))
  await Promise.resolve()
  deferred.resolve("second")
  await secondLoad

  expect(queryClient.read(queryKey, loadSession, ["ses_1"])).toBe("second")
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

test("QueryClient.refetchActiveQueries skips queries that opt out of window reactivation", async () => {
  let activeDeferred = createDeferred<string>()
  let skippedDeferred = createDeferred<string>()
  const activeQueryNotifications: string[] = []
  const activeRefetchSettled = createDeferred<void>()
  let isWaitingForActiveRefetch = false
  const queryClient = new QueryClient()
  const loadActiveSession = vi.fn((_sessionId: string) => activeDeferred.promise)
  const loadSkippedSession = vi.fn((_sessionId: string) => skippedDeferred.promise)
  const activeQueryKey = queryClient.getQueryKey(loadActiveSession, ["ses_active"])
  const skippedQueryKey = queryClient.getQueryKey(loadSkippedSession, ["ses_skipped"])

  const firstActiveLoad = waitForSuspendedRead(() =>
    queryClient.read(activeQueryKey, loadActiveSession, ["ses_active"]),
  )
  const firstSkippedLoad = waitForSuspendedRead(() =>
    queryClient.read(skippedQueryKey, loadSkippedSession, ["ses_skipped"], {
      refetchOnWindowReactivate: false,
    }),
  )

  queryClient.subscribe(activeQueryKey, () => {
    activeQueryNotifications.push("update")

    if (isWaitingForActiveRefetch && activeQueryNotifications.length === 2) {
      activeRefetchSettled.resolve()
    }
  })
  queryClient.subscribe(skippedQueryKey, () => {})

  await Promise.resolve()
  activeDeferred.resolve("active:first")
  skippedDeferred.resolve("skipped:first")
  await firstActiveLoad
  await firstSkippedLoad

  activeDeferred = createDeferred<string>()
  skippedDeferred = createDeferred<string>()
  loadActiveSession.mockReturnValueOnce(activeDeferred.promise)
  loadSkippedSession.mockReturnValueOnce(skippedDeferred.promise)
  isWaitingForActiveRefetch = true

  queryClient.refetchActiveQueries()
  await Promise.resolve()

  expect(loadActiveSession).toHaveBeenCalledTimes(2)
  expect(loadSkippedSession).toHaveBeenCalledTimes(1)

  activeDeferred.resolve("active:second")
  await activeRefetchSettled.promise

  expect(queryClient.read(activeQueryKey, loadActiveSession, ["ses_active"])).toBe("active:second")
  expect(
    queryClient.read(skippedQueryKey, loadSkippedSession, ["ses_skipped"], {
      refetchOnWindowReactivate: false,
    }),
  ).toBe("skipped:first")
})
