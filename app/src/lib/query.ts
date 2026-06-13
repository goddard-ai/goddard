import hashSum from "hash-sum"
import { useEffect, useState } from "preact/hooks"

type QueryArgs = readonly any[]
type QueryFunction<TArgs extends QueryArgs = QueryArgs, TData = unknown> = (
  ...args: TArgs
) => Promise<TData>
type AnyQueryFunction = QueryFunction<any, any>
type EmptyQueryResult = Record<string, never>
type DisabledQuery = null | EmptyQueryResult
type QueryInput = AnyQueryFunction | DisabledQuery
type QueryOptions = {
  refetchOnWindowReactivate?: boolean
}

type QueryRequest<TQueryFn extends QueryInput = QueryInput> = QueryOptions & {
  params: TQueryFn extends AnyQueryFunction ? Parameters<TQueryFn> : null
}

type QueryEntry = {
  args: QueryArgs
  data: unknown
  error: unknown
  hasData: boolean
  injectionId: number | null
  promise: Promise<unknown> | null
  queryFn: AnyQueryFunction
  refetchOnWindowReactivate: boolean
  stale: boolean
  subscribers: Set<() => void>
}

type QueryDescriptor<TQueryFn extends QueryInput = QueryInput> = {
  options: Required<QueryOptions>
  params: TQueryFn extends AnyQueryFunction ? Parameters<TQueryFn> : null
  queryFn: TQueryFn
}

type QueryResult<TQueryFn extends QueryInput> = TQueryFn extends AnyQueryFunction
  ? Awaited<ReturnType<TQueryFn>>
  : never

type QueryResults<TQueries extends readonly QueryDescriptor[]> = {
  [TKey in keyof TQueries]: QueryResult<TQueries[TKey]["queryFn"]>
}

type QueryBuilder = {
  <TQueryFn extends AnyQueryFunction>(
    queryFn: TQueryFn,
    request: QueryRequest<TQueryFn>,
  ): QueryDescriptor<TQueryFn>
  <TQueryFn extends DisabledQuery>(
    queryFn: TQueryFn,
    request: QueryRequest<TQueryFn>,
  ): QueryDescriptor<TQueryFn>
}

const defaultQueryOptions = {
  refetchOnWindowReactivate: true,
} satisfies Required<QueryOptions>

/**
 * Detects the explicit disabled-query sentinels supported by the query hooks.
 */
function isEnabledQuery(queryFn: QueryInput): queryFn is AnyQueryFunction {
  return queryFn !== null && !isEmptyQueryObject(queryFn)
}

/**
 * Distinguishes the `{}` disabled-query sentinel from enabled query functions.
 */
function isEmptyQueryObject(value: QueryInput): value is EmptyQueryResult {
  return (
    typeof value === "object" &&
    value !== null &&
    Object.getPrototypeOf(value) === Object.prototype &&
    Object.keys(value).length === 0
  )
}

function createQueryDescriptor<TQueryFn extends QueryInput>(
  queryFn: TQueryFn,
  request: QueryRequest<TQueryFn>,
) {
  return {
    options: {
      ...defaultQueryOptions,
      refetchOnWindowReactivate:
        request.refetchOnWindowReactivate ?? defaultQueryOptions.refetchOnWindowReactivate,
    },
    params: request.params,
    queryFn,
  } satisfies QueryDescriptor<TQueryFn>
}

/**
 * Stores query results by query function plus argument tuple and drives the local Suspense cache.
 */
export class QueryClient {
  private entries = new Map<string, QueryEntry>()
  private entryKeysByFunction = new WeakMap<AnyQueryFunction, Set<string>>()
  private functionIds = new WeakMap<AnyQueryFunction, string>()
  private nextFunctionId = 0
  private nextInjectionId = 0

  /**
   * Returns the stable cache key for one query function and argument tuple.
   */
  getQueryKey<TQueryFn extends AnyQueryFunction>(queryFn: TQueryFn, args: Parameters<TQueryFn>) {
    return hashSum([this.getFunctionId(queryFn), args])
  }

  /**
   * Reads the current cache snapshot without throwing, kicking off a fetch when the entry is stale
   * or missing.
   */
  getSnapshot<TQueryFn extends AnyQueryFunction>(
    queryKey: string,
    queryFn: TQueryFn,
    args: Parameters<TQueryFn>,
    options: Required<QueryOptions> = defaultQueryOptions,
  ) {
    const entry = this.ensureEntry(queryKey, queryFn, args, options)
    entry.refetchOnWindowReactivate = options.refetchOnWindowReactivate

    if (entry.stale || (!entry.hasData && !entry.promise && entry.error === null)) {
      void this.fetchEntry(entry, entry.hasData)
    }

    return {
      data: entry.data as Awaited<ReturnType<TQueryFn>> | undefined,
      error: entry.error,
      hasData: entry.hasData,
      promise: entry.promise,
      shouldSuspend: entry.promise !== null && !entry.hasData,
    }
  }

  /**
   * Returns cached data for one query and suspends only while the first load is still pending.
   */
  read<TQueryFn extends AnyQueryFunction>(
    queryKey: string,
    queryFn: TQueryFn,
    args: Parameters<TQueryFn>,
    options: Required<QueryOptions> = defaultQueryOptions,
  ) {
    const snapshot = this.getSnapshot(queryKey, queryFn, args, options)

    if (snapshot.error !== null && !snapshot.hasData) {
      throw snapshot.error
    }

    if (snapshot.shouldSuspend && snapshot.promise) {
      throw snapshot.promise
    }

    return snapshot.data!
  }

  /**
   * Registers a listener for one existing query entry key and returns the unsubscribe callback.
   */
  subscribe(queryKey: string, subscriber: () => void) {
    const entry = this.getEntry(queryKey)
    const wasInactive = entry.subscribers.size === 0
    entry.subscribers.add(subscriber)

    if (wasInactive && entry.hasData && !entry.promise && !this.isInjected(entry)) {
      void this.fetchEntry(entry, true)
    }

    return () => {
      entry.subscribers.delete(subscriber)
    }
  }

  /**
   * Marks one cached query, or every query for the same function, stale and refetches active
   * subscribers immediately.
   */
  invalidate<TQueryFn extends AnyQueryFunction>(queryFn: TQueryFn, args?: Parameters<TQueryFn>) {
    if (args) {
      const entry = this.entries.get(this.getQueryKey(queryFn, args))

      if (entry) {
        this.invalidateEntry(entry)
      }

      return
    }

    for (const key of this.entryKeysByFunction.get(queryFn) ?? []) {
      const entry = this.entries.get(key)

      if (entry) {
        this.invalidateEntry(entry)
      }
    }
  }

  /**
   * Drops one inactive cached query so the next read waits for a fresh first result.
   * Injected query data stays active until its cleanup runs.
   */
  evict<TQueryFn extends AnyQueryFunction>(queryFn: TQueryFn, args: Parameters<TQueryFn>) {
    const queryKey = this.getQueryKey(queryFn, args)
    const entry = this.entries.get(queryKey)

    if (!entry) {
      return
    }

    if (this.isInjected(entry)) {
      return
    }

    if (entry.subscribers.size > 0) {
      this.invalidateEntry(entry)
      return
    }

    this.entries.delete(queryKey)
    this.entryKeysByFunction.get(queryFn)?.delete(queryKey)
  }

  /**
   * Refreshes every query that is currently observed by mounted UI.
   */
  refetchActiveQueries() {
    for (const entry of this.entries.values()) {
      if (
        entry.refetchOnWindowReactivate &&
          entry.subscribers.size > 0 &&
          !entry.promise &&
          !this.isInjected(entry)
      ) {
        void this.fetchEntry(entry, entry.hasData)
      }
    }
  }

  /**
   * Temporarily injects one query result and returns cleanup that restores normal fetching.
   */
  injectData<TQueryFn extends AnyQueryFunction>(
    queryFn: TQueryFn,
    args: Parameters<TQueryFn>,
    data: Awaited<ReturnType<TQueryFn>>,
  ) {
    const queryKey = this.getQueryKey(queryFn, args)
    const existingEntry = this.entries.get(queryKey)
    const entry = existingEntry ?? this.ensureEntry(queryKey, queryFn, args, defaultQueryOptions)
    const injectionId = ++this.nextInjectionId
    const previousEntry = {
      data: entry.data,
      error: entry.error,
      hasData: entry.hasData,
      promise: entry.promise,
      refetchOnWindowReactivate: entry.refetchOnWindowReactivate,
      stale: entry.stale,
    }

    entry.data = data
    entry.error = null
    entry.hasData = true
    entry.injectionId = injectionId
    entry.promise = null
    entry.stale = false
    this.notify(entry)

    return () => {
      if (entry.injectionId !== injectionId) {
        return
      }

      entry.injectionId = null

      if (!existingEntry) {
        if (entry.subscribers.size === 0) {
          this.entries.delete(queryKey)
          this.entryKeysByFunction.get(queryFn)?.delete(queryKey)
          return
        }

        entry.data = undefined
        entry.error = null
        entry.hasData = false
        entry.promise = null
        entry.stale = true
        this.notify(entry)
        return
      }

      entry.data = previousEntry.data
      entry.error = previousEntry.error
      entry.hasData = previousEntry.hasData
      entry.promise = null
      entry.refetchOnWindowReactivate = previousEntry.refetchOnWindowReactivate
      entry.stale = previousEntry.promise ? true : previousEntry.stale
      this.notify(entry)
    }
  }

  private fetchEntry(entry: QueryEntry, background: boolean) {
    if (this.isInjected(entry)) {
      return Promise.resolve(entry.data)
    }

    if (entry.promise) {
      return entry.promise
    }

    entry.error = null
    entry.stale = false

    const promise = Promise.resolve().then(() => entry.queryFn(...entry.args))
    entry.promise = promise

    promise.then(
      (data) => {
        if (entry.promise !== promise) {
          return
        }

        entry.data = data
        entry.hasData = true
        entry.promise = null
        this.notify(entry)

        if (entry.stale) {
          void this.fetchEntry(entry, background)
        }
      },
      (error) => {
        if (entry.promise !== promise) {
          return
        }

        entry.promise = null

        if (!entry.hasData) {
          entry.error = error
        }

        this.notify(entry)

        if (entry.stale) {
          void this.fetchEntry(entry, entry.hasData)
        }
      },
    )

    return promise
  }

  private getEntry(queryKey: string) {
    const entry = this.entries.get(queryKey)

    if (!entry) {
      throw new Error(`Missing query entry for key ${queryKey}.`)
    }

    return entry
  }

  private ensureEntry<TQueryFn extends AnyQueryFunction>(
    queryKey: string,
    queryFn: TQueryFn,
    args: Parameters<TQueryFn>,
    options: Required<QueryOptions>,
  ) {
    const existingEntry = this.entries.get(queryKey)

    if (existingEntry) {
      return existingEntry
    }

    const entry: QueryEntry = {
      args,
      data: undefined,
      error: null,
      hasData: false,
      injectionId: null,
      promise: null,
      queryFn,
      refetchOnWindowReactivate: options.refetchOnWindowReactivate,
      stale: true,
      subscribers: new Set(),
    }

    this.entries.set(queryKey, entry)
    this.getFunctionEntryKeys(queryFn).add(queryKey)
    return entry
  }

  private getFunctionEntryKeys(queryFn: AnyQueryFunction) {
    const existingEntryKeys = this.entryKeysByFunction.get(queryFn)

    if (existingEntryKeys) {
      return existingEntryKeys
    }

    const nextEntryKeys = new Set<string>()
    this.entryKeysByFunction.set(queryFn, nextEntryKeys)
    return nextEntryKeys
  }

  private getFunctionId(queryFn: AnyQueryFunction) {
    const existingId = this.functionIds.get(queryFn)

    if (existingId) {
      return existingId
    }

    this.nextFunctionId += 1
    const nextId = `query:${this.nextFunctionId}`
    this.functionIds.set(queryFn, nextId)
    return nextId
  }

  private invalidateEntry(entry: QueryEntry) {
    entry.stale = true

    if (entry.subscribers.size > 0 && !entry.promise && !this.isInjected(entry)) {
      void this.fetchEntry(entry, entry.hasData)
    }
  }

  private isInjected(entry: QueryEntry) {
    return entry.injectionId !== null
  }

  private notify(entry: QueryEntry) {
    for (const subscriber of entry.subscribers) {
      subscriber()
    }
  }
}

export const queryClient = new QueryClient()

let stopQueryWindowReactivationRefetch: (() => void) | null = null

/**
 * Installs one global listener set that refreshes active queries after the desktop view becomes
 * visible or focused again.
 */
export function startQueryWindowReactivationRefetch() {
  if (stopQueryWindowReactivationRefetch) {
    return
  }

  let scheduledRefetchFrame: number | null = null

  function scheduleActiveQueryRefetch() {
    if (document.visibilityState === "hidden") {
      return
    }

    if (scheduledRefetchFrame !== null) {
      return
    }

    scheduledRefetchFrame = window.requestAnimationFrame(() => {
      scheduledRefetchFrame = null
      queryClient.refetchActiveQueries()
    })
  }

  function handleVisibilityChange() {
    if (document.visibilityState === "visible") {
      scheduleActiveQueryRefetch()
    }
  }

  window.addEventListener("focus", scheduleActiveQueryRefetch)
  document.addEventListener("visibilitychange", handleVisibilityChange)

  stopQueryWindowReactivationRefetch = () => {
    window.removeEventListener("focus", scheduleActiveQueryRefetch)
    document.removeEventListener("visibilitychange", handleVisibilityChange)

    if (scheduledRefetchFrame !== null) {
      window.cancelAnimationFrame(scheduledRefetchFrame)
      scheduledRefetchFrame = null
    }

    stopQueryWindowReactivationRefetch = null
  }
}

import.meta.hot.dispose(() => {
  stopQueryWindowReactivationRefetch?.()
})

/**
 * Reads one cached query and returns the resolved data directly, or returns `null` / `{}` when the
 * query is explicitly disabled with one of those sentinels.
 *
 * The hook suspends during the initial load, then keeps returning the last resolved value while
 * later refetches run in the background.
 */
export function useQuery<TQueryFn extends QueryInput>(
  queryFn: TQueryFn,
  request: QueryRequest<TQueryFn>,
): QueryResult<TQueryFn> {
  const [, setVersion] = useState(0)
  const descriptor = createQueryDescriptor(queryFn, request)

  const queryKey = isEnabledQuery(descriptor.queryFn)
    ? queryClient.getQueryKey(
        descriptor.queryFn,
        descriptor.params as Parameters<typeof descriptor.queryFn>,
      )
    : null

  useEffect(() => {
    if (queryKey)
      return queryClient.subscribe(queryKey, () => {
        setVersion((version) => version + 1)
      })
  }, [queryKey])

  if (isEnabledQuery(descriptor.queryFn)) {
    return queryClient.read(
      queryKey!,
      descriptor.queryFn,
      descriptor.params as Parameters<typeof descriptor.queryFn>,
      descriptor.options,
    )
  }

  return descriptor.queryFn as QueryResult<TQueryFn>
}

/**
 * Reads multiple cached queries from an ordered descriptor list and returns the resolved data in
 * the same order, preserving disabled-query sentinels in their original positions.
 */
export function useQueries<const TQueries extends readonly QueryDescriptor[]>(
  createQueries: (query: QueryBuilder) => TQueries,
) {
  const [, setVersion] = useState(0)
  const queries = createQueries(createQueryDescriptor as QueryBuilder)
  const queryKeys = queries.map((descriptor) =>
    isEnabledQuery(descriptor.queryFn)
      ? queryClient.getQueryKey(
          descriptor.queryFn,
          descriptor.params as Parameters<typeof descriptor.queryFn>,
        )
      : null,
  )

  useEffect(() => {
    const unsubscribers = queryKeys.flatMap((queryKey) => {
      if (!queryKey) {
        return []
      }
      return queryClient.subscribe(queryKey, () => {
        setVersion((version) => version + 1)
      })
    })

    return () => {
      for (const unsubscribe of unsubscribers) {
        unsubscribe()
      }
    }
  }, queryKeys)

  const data: any[] = []
  const pendingPromises: Promise<unknown>[] = []

  for (const [index, descriptor] of queries.entries()) {
    if (!isEnabledQuery(descriptor.queryFn)) {
      data[index] = descriptor.queryFn
      continue
    }

    const queryKey = queryKeys[index]
    if (!queryKey) {
      throw new Error("Missing query key for enabled query.")
    }

    const snapshot = queryClient.getSnapshot(
      queryKey,
      descriptor.queryFn,
      descriptor.params as Parameters<typeof descriptor.queryFn>,
      descriptor.options,
    )

    if (snapshot.error !== null && !snapshot.hasData) {
      throw snapshot.error
    }

    if (snapshot.shouldSuspend && snapshot.promise) {
      pendingPromises.push(snapshot.promise)
    }

    if (snapshot.hasData) {
      data[index] = snapshot.data
    }
  }

  if (pendingPromises.length > 0) {
    throw Promise.all(pendingPromises)
  }

  return data as QueryResults<TQueries>
}
