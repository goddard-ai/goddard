import type { Store } from '@octane-xplat/ui';

/** Minimal keyed query cache standing in for react-query: entries are
 * created on first subscribe, refetch when invalidated while observed, and
 * keep serving their last data (stale-while-revalidate) like the mobile
 * app's hooks did. */

export interface DaemonQuerySnapshot<T> {
  status: 'pending' | 'success' | 'error';
  data: T | undefined;
  error: unknown;
  dataUpdatedAt: number;
  /** A fetch is in flight (initial or refetch). */
  isFetching: boolean;
}

export interface DaemonQueryEntry<T> extends Store<DaemonQuerySnapshot<T>> {
  refetch: () => Promise<T | undefined>;
  /** Number of active hook subscriptions — invalidation only refetches
   * observed entries. */
  observers: number;
  fetcher: () => Promise<T>;
  staleTime: number;
}

const entries = new Map<string, DaemonQueryEntry<unknown>>();

export function serializedKey(key: readonly unknown[]): string {
  return JSON.stringify(key);
}

export function daemonQueryEntry<T>(
  key: readonly unknown[],
  fetcher: () => Promise<T>,
  staleTime = 1_000,
): DaemonQueryEntry<T> {
  const id = serializedKey(key);
  const existing = entries.get(id) as DaemonQueryEntry<T> | undefined;
  if (existing) {return existing;}

  let generation = 0;
  let listeners = new Set<() => void>();
  const entry: DaemonQueryEntry<T> = {
    observers: 0,
    fetcher,
    staleTime,
    get: () => snapshot,
    subscribe: (notify) => {
      listeners.add(notify);
      entry.observers = listeners.size;
      maybeFetch();
      return () => {
        listeners.delete(notify);
        entry.observers = listeners.size;
      };
    },
    set: (next) => {
      snapshot = typeof next === 'function' ? next(snapshot) : next;
      for (const notify of listeners) {notify();}
    },
    refetch: async () => {
      const attempt = ++generation;
      snapshot = { ...snapshot, isFetching: true };
      for (const notify of listeners) {notify();}
      try {
        const data = await entry.fetcher();
        if (attempt !== generation || !entries.has(id)) {return data;}
        snapshot = {
          status: 'success',
          data,
          error: undefined,
          dataUpdatedAt: Date.now(),
          isFetching: false,
        };
      } catch (error) {
        if (attempt !== generation || !entries.has(id)) {return undefined;}
        snapshot = { ...snapshot, status: 'error', error, isFetching: false };
      }

      for (const notify of listeners) {notify();}
      return snapshot.data;
    },
  };

  let snapshot: DaemonQuerySnapshot<T> = {
    status: 'pending',
    data: undefined,
    error: undefined,
    dataUpdatedAt: 0,
    isFetching: false,
  };

  function maybeFetch() {
    if (snapshot.isFetching) {return;}
    if (snapshot.status === 'pending') {
      void entry.refetch();
      return;
    }

    if (entry.staleTime !== Infinity && Date.now() - snapshot.dataUpdatedAt > entry.staleTime) {
      void entry.refetch();
    }
  }

  entries.set(id, entry as DaemonQueryEntry<unknown>);
  return entry;
}

/** Push data straight into the cache (daemon broadcasts, updater fns). */
export function setDaemonQueryData<T>(key: readonly unknown[], data: T | ((current: T | undefined) => T | undefined)) {
  const id = serializedKey(key);
  const entry = entries.get(id) as DaemonQueryEntry<T> | undefined;
  if (!entry) {return;}
  entry.set({
    status: 'success',
    data: typeof data === 'function'
      ? (data as (c: T | undefined) => T | undefined)(entry.get().data) ?? entry.get().data
      : data,
    error: undefined,
    dataUpdatedAt: Date.now(),
    isFetching: false,
  });
}

export function getDaemonQueryData<T>(key: readonly unknown[]): T | undefined {
  return (entries.get(serializedKey(key)) as DaemonQueryEntry<T> | undefined)?.get().data;
}

/** Refetch observed entries under ['daemon', profileId, kind...] matching
 * the kind predicate — the invalidateQueries({predicate}) port. */
export function invalidateDaemonQueries(
  profileId: string,
  kind: (kind: string) => boolean,
) {
  for (const [id, entry] of entries) {
    if (entry.observers === 0) {continue;}
    let parsed: unknown[];
    try {
      parsed = JSON.parse(id);
    } catch {
      continue;
    }

    if (parsed[0] === 'daemon' && parsed[1] === profileId && typeof parsed[2] === 'string' && kind(parsed[2])) {
      void entry.refetch();
    }
  }
}

/** Drop every cached entry for a removed profile. */
export function removeDaemonQueries(profileId: string) {
  for (const [id] of entries) {
    let parsed: unknown[];
    try {
      parsed = JSON.parse(id);
    } catch {
      continue;
    }

    if (parsed[0] === 'daemon' && parsed[1] === profileId) {entries.delete(id);}
  }
}

/** Refetch observed entries whose key starts with `prefix` and passes
 * `predicate` (react-query invalidateQueries({queryKey, predicate})). */
export function invalidateDaemonQueriesMatching(
  prefix: readonly unknown[],
  predicate?: (query: { queryKey: readonly unknown[] }) => boolean,
) {
  const prefixId = JSON.stringify(prefix).slice(0, -1);
  for (const [id, entry] of entries) {
    if (entry.observers === 0) {continue;}
    let parsed: unknown[];
    try {
      parsed = JSON.parse(id);
    } catch {
      continue;
    }

    const keyMatches = parsed.length === prefix.length
      ? JSON.stringify(parsed) === JSON.stringify(prefix)
      : id.startsWith(prefixId);

    if (!keyMatches) {continue;}
    if (predicate && !predicate({ queryKey: parsed })) {continue;}
    void entry.refetch();
  }
}

/** Remove one exact cached entry (react-query removeQueries({queryKey})). */
export function removeDaemonQuery(key: readonly unknown[]) {
  entries.delete(serializedKey(key));
}
