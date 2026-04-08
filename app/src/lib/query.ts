import hashSum from "hash-sum"
import type { ComponentChildren } from "preact"
import { createContext, createElement } from "preact"
import { useContext, useEffect, useState } from "preact/hooks"

type QueryArgs = readonly unknown[]
type QueryFunction<TArgs extends QueryArgs = QueryArgs, TData = unknown> = (
  ...args: TArgs
) => Promise<TData>
type AnyQueryFunction = QueryFunction<any, any>

type QueryEntry = {
  args: QueryArgs
  data: unknown
  error: unknown
  hasData: boolean
  promise: Promise<unknown> | null
  queryFn: AnyQueryFunction
  stale: boolean
  subscribers: Set<() => void>
}

type QueryDescriptor<TQueryFn extends AnyQueryFunction = AnyQueryFunction> = readonly [
  TQueryFn,
  Parameters<TQueryFn>,
]

type QueryResults<TQueries extends Record<string, QueryDescriptor>> = {
  [TKey in keyof TQueries]: Awaited<ReturnType<TQueries[TKey][0]>>
}

const queryClientContext = createContext<QueryClient | null>(null)

function requireQueryClient(value: QueryClient | null) {
  if (!value) {
    throw new Error("queryClientContext is missing.")
  }

  return value
}

/**
 * Stores query results by query function plus argument tuple and drives the local Suspense cache.
 */
export class QueryClient {
  private entries = new Map<string, QueryEntry>()
  private entryKeysByFunction = new WeakMap<AnyQueryFunction, Set<string>>()
  private functionIds = new WeakMap<AnyQueryFunction, string>()
  private nextFunctionId = 0

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
  getSnapshot<TQueryFn extends AnyQueryFunction>(queryFn: TQueryFn, args: Parameters<TQueryFn>) {
    const entry = this.getEntry(queryFn, args)

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
  read<TQueryFn extends AnyQueryFunction>(queryFn: TQueryFn, args: Parameters<TQueryFn>) {
    const snapshot = this.getSnapshot(queryFn, args)

    if (snapshot.error !== null && !snapshot.hasData) {
      throw snapshot.error
    }

    if (snapshot.shouldSuspend && snapshot.promise) {
      throw snapshot.promise
    }

    return snapshot.data as Awaited<ReturnType<TQueryFn>>
  }

  /**
   * Registers a listener for one query entry and returns the unsubscribe callback.
   */
  subscribe<TQueryFn extends AnyQueryFunction>(
    queryFn: TQueryFn,
    args: Parameters<TQueryFn>,
    subscriber: () => void,
  ) {
    const entry = this.getEntry(queryFn, args)
    entry.subscribers.add(subscriber)

    return () => {
      entry.subscribers.delete(subscriber)
    }
  }

  /**
   * Refreshes one cached query in place while preserving the last resolved value.
   */
  refetch<TQueryFn extends AnyQueryFunction>(queryFn: TQueryFn, args: Parameters<TQueryFn>) {
    const entry = this.getEntry(queryFn, args)
    void this.fetchEntry(entry, entry.hasData)
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

  private fetchEntry(entry: QueryEntry, background: boolean) {
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

  private getEntry<TQueryFn extends AnyQueryFunction>(
    queryFn: TQueryFn,
    args: Parameters<TQueryFn>,
  ) {
    const key = this.getQueryKey(queryFn, args)
    const existingEntry = this.entries.get(key)

    if (existingEntry) {
      return existingEntry
    }

    const entry: QueryEntry = {
      args,
      data: undefined,
      error: null,
      hasData: false,
      promise: null,
      queryFn,
      stale: true,
      subscribers: new Set(),
    }

    this.entries.set(key, entry)
    this.getFunctionEntryKeys(queryFn).add(key)
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

    if (entry.subscribers.size > 0 && !entry.promise) {
      void this.fetchEntry(entry, entry.hasData)
    }
  }

  private notify(entry: QueryEntry) {
    for (const subscriber of entry.subscribers) {
      subscriber()
    }
  }
}

/**
 * Supplies one shared query client to hooks rendered below it.
 */
export function QueryClientProvider(props: { children: ComponentChildren; client: QueryClient }) {
  return createElement(queryClientContext.Provider, { value: props.client }, props.children)
}

/**
 * Returns the shared query client for manual invalidation and direct cache interaction.
 */
export function useQueryClient() {
  return requireQueryClient(useContext(queryClientContext))
}

/**
 * Reads one cached query and returns a `[data, refetch]` tuple.
 *
 * The hook suspends during the initial load, then keeps returning the last resolved value while
 * later refetches run in the background.
 */
export function useQuery<TQueryFn extends AnyQueryFunction>(
  queryFn: TQueryFn,
  args: Parameters<TQueryFn>,
) {
  const queryClient = useQueryClient()
  const [, setVersion] = useState(0)
  const queryKey = queryClient.getQueryKey(queryFn, args)

  useEffect(() => {
    return queryClient.subscribe(queryFn, args, () => {
      setVersion((version) => version + 1)
    })
  }, [queryClient, queryFn, queryKey])

  const data = queryClient.read(queryFn, args)

  return [
    data,
    () => {
      queryClient.refetch(queryFn, args)
    },
  ] as const
}

/**
 * Reads multiple cached queries from a keyed object and returns `[data, refetch]`, where `refetch`
 * can refresh one key or the whole set.
 */
export function useQueries<TQueries extends Record<string, QueryDescriptor>>(queries: TQueries) {
  const queryClient = useQueryClient()
  const [, setVersion] = useState(0)
  const entries = Object.entries(queries) as Array<
    [keyof TQueries & string, TQueries[keyof TQueries]]
  >
  const subscriptionKey = hashSum(
    entries.map(([key, [queryFn, args]]) => [key, queryClient.getQueryKey(queryFn, args)]),
  )

  useEffect(() => {
    const unsubscribers = entries.map(([, [queryFn, args]]) =>
      queryClient.subscribe(queryFn, args, () => {
        setVersion((version) => version + 1)
      }),
    )

    return () => {
      for (const unsubscribe of unsubscribers) {
        unsubscribe()
      }
    }
  }, [queryClient, subscriptionKey])

  const data = {} as QueryResults<TQueries>
  let pendingPromise: Promise<unknown> | null = null

  for (const [key, [queryFn, args]] of entries) {
    const snapshot = queryClient.getSnapshot(queryFn, args)

    if (snapshot.error !== null && !snapshot.hasData) {
      throw snapshot.error
    }

    if (snapshot.shouldSuspend && snapshot.promise && pendingPromise === null) {
      pendingPromise = snapshot.promise
    }

    if (snapshot.hasData) {
      ;(data as Record<string, unknown>)[key] = snapshot.data
    }
  }

  if (pendingPromise) {
    throw pendingPromise
  }

  return [
    data,
    (key?: keyof TQueries & string) => {
      if (key) {
        const query = queries[key]

        if (query) {
          queryClient.refetch(query[0], query[1])
        }

        return
      }

      for (const [, [queryFn, args]] of entries) {
        queryClient.refetch(queryFn, args)
      }
    },
  ] as const
}
