import { useStore } from '@octane-xplat/ui';
import { useEffect, useState } from 'octane';

import { daemonState, bootstrapDaemon, reconnect, saveProfile, selectProfile, removeProfile, disconnect } from './daemon-store';
import { daemonQueryEntry, getDaemonQueryData } from './daemon-data';
import { installAttentionFeed } from './attention-feed';

/** The reference app's useDaemon() context value, minus provider plumbing:
 * actions are module functions, state is the shared store. */
export function useDaemon() {
  const state = useStore(daemonState);
  return {
    ...state,
    saveProfile,
    selectProfile,
    removeProfile,
    reconnect,
    disconnect,
  };
}

/** Kick daemon bootstrap once per app — call unconditionally in the root
 * component. */
export function useDaemonBootstrap() {
  useEffect(() => {
    bootstrapDaemon();
    // The backgrounded-notification feed rides the same query stream.
    installAttentionFeed();
  }, []);
}

export interface DaemonQueryResult<T> {
  data: T | undefined;
  error: unknown;
  isPending: boolean;
  isFetching: boolean;
  dataUpdatedAt: number;
  refetch: () => Promise<T | undefined>;
}

/** Subscribed cached query — the react-query useQuery port. Subscribing
 * (enabled) is what arms the entry's fetch and marks it observed for
 * invalidation-driven refetches. */
export function useDaemonQuery<T>(
  key: readonly unknown[],
  fetcher: () => Promise<T>,
  options?: { staleTime?: number; enabled?: boolean },
): DaemonQueryResult<T> {
  const enabled = options?.enabled !== false;
  const keyId = JSON.stringify(key);
  const entry = daemonQueryEntry(key, fetcher, options?.staleTime);
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!enabled) {return;}
    return entry.subscribe(() => setTick((tick) => tick + 1));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, keyId]);

  const snapshot = entry.get();
  return {
    data: snapshot.data,
    error: snapshot.error,
    isPending: enabled && snapshot.status === 'pending',
    isFetching: enabled && snapshot.isFetching,
    dataUpdatedAt: snapshot.dataUpdatedAt,
    refetch: entry.refetch,
  };
}

export { getDaemonQueryData };
