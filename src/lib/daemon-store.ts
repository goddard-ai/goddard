import { WakuClient } from '@waku/client';
import { bindBossSecondary } from './boss-secondary-store';
import { createStore } from '@octane-xplat/ui/data';
import { subscribeAppForeground } from './app-foreground';

import { daemonKeys, normalizeDaemonSettings } from './daemon-api';
import { hydratePersistentStorage } from './composer-preferences-store';
import { DaemonLink, type DaemonOutage } from './daemon-link';
import {
  normalizeDaemonProfile,
  isPrivateDaemonAddress,
  type DaemonProfile,
  type DaemonProfileInput,
} from './daemon-profile';

import {
  deleteDaemonToken,
  readActiveDaemonId,
  readDaemonProfiles,
  readDaemonToken,
  writeActiveDaemonId,
  writeDaemonProfiles,
  writeDaemonToken,
} from './daemon-storage';

import { daemonSocketFactory } from './socket';
import { randomUUID } from './uuid';
import { invalidateDaemonQueries, removeDaemonQueries, setDaemonQueryData } from './daemon-data';

export type { DaemonOutage } from './daemon-link';

/**
 * - `booting`: saved daemons are still loading.
 * - `disconnected`: no daemon is selected.
 * - `connecting`: the selected daemon's first attempt is in flight.
 * - `connected`: live.
 * - `reconnecting`: the link is down and retrying on its own; `outage`
 *   reports its progress.
 * - `error`: the link is down and waiting on the user; `error` says why.
 */
export type ConnectionPhase =
  | 'booting'
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'error';

export interface DaemonState {
  phase: ConnectionPhase;
  error: string | null;
  outage: DaemonOutage | null;
  /** Successful connections to the selected daemon. Past the first, screens
   * refresh what the daemon may have changed while the link was down. */
  connections: number;
  profiles: DaemonProfile[];
  activeProfile: DaemonProfile | null;
  client: WakuClient | null;
}

const IDLE = {
  phase: 'disconnected' as ConnectionPhase,
  error: null,
  outage: null,
  connections: 0,
};

export const daemonState = createStore<DaemonState>({
  ...IDLE,
  phase: 'booting',
  profiles: [],
  activeProfile: null,
  client: null,
});

// Module-scope connection bookkeeping — mirrors the reference context's refs.
let profilesRef: DaemonProfile[] = [];
let activeProfileIdRef: string | null = null;
let linkRef: DaemonLink | null = null;
let generation = 0;
let unsubscribeLink: (() => void) | null = null;
let unsubscribeTaskState: (() => void) | null = null;
let unsubscribeSettings: (() => void) | null = null;
let unsubscribeBossSecondary: (() => void) | null = null;
let bootstrapped = false;

function setStatus(status: { phase: ConnectionPhase; error: string | null; outage: DaemonOutage | null; connections: number }) {
  daemonState.update((prev) => ({ ...prev, ...status }));
}

async function commitProfiles(next: DaemonProfile[]) {
  profilesRef = next;
  daemonState.update((prev) => ({
    ...prev,
    profiles: next,
    activeProfile: next.find((item) => item.id === activeProfileIdRef) ?? null,
  }));

  await writeDaemonProfiles(next);
}

function closeCurrentLink(updateState = true) {
  unsubscribeLink?.();
  unsubscribeLink = null;
  unsubscribeTaskState?.();
  unsubscribeTaskState = null;
  unsubscribeSettings?.();
  unsubscribeSettings = null;
  unsubscribeBossSecondary?.();
  unsubscribeBossSecondary = null;
  const current = linkRef;
  linkRef = null;
  if (updateState) {daemonState.update((prev) => ({ ...prev, client: null }));}
  current?.close();
}

/** Recency and cache upkeep for every (re)connection. */
function noteConnected(profileId: string, connections: number) {
  const connectedAt = Date.now();
  const updated = profilesRef.map((item) =>
    item.id === profileId ? { ...item, lastConnectedAt: connectedAt } : item,
  );

  profilesRef = updated;
  daemonState.update((prev) => ({ ...prev, profiles: updated }));
  void writeDaemonProfiles(updated).catch(() => {
    // A recency metadata write must not turn a live connection into an error state.
  });

  if (connections > 1) {
    // Task-state and session change notifications sent while the link
    // was down are gone, so refetch what screens are showing. Followed
    // runtimes replay their missed events on their own.
    invalidateDaemonQueries(profileId, (kind) => kind === 'task-state' || kind === 'session');
  }
}

async function activate(profileId: string, candidates = profilesRef): Promise<boolean> {
  const profile = candidates.find((item) => item.id === profileId);
  if (!profile) {return false;}

  const attempt = ++generation;
  closeCurrentLink();
  activeProfileIdRef = profile.id;
  daemonState.update((prev) => ({
    ...prev,
    activeProfile: profile,
    ...IDLE,
    phase: 'connecting',
  }));

  try {
    await writeActiveDaemonId(profile.id);
  } catch (cause) {
    if (generation !== attempt) {return false;}
    setStatus({
      ...IDLE,
      phase: 'error',
      error: errorMessage(cause, 'Couldn’t remember the selected daemon'),
    });

    return false;
  }

  let token: string | null;
  try {
    token = await readDaemonToken(profile.id);
  } catch (cause) {
    if (generation !== attempt) {return false;}
    setStatus({
      ...IDLE,
      phase: 'error',
      error: errorMessage(cause, 'Couldn’t read this daemon token'),
    });

    return false;
  }

  if (generation !== attempt) {return false;}
  if (!token) {
    setStatus({
      ...IDLE,
      phase: 'error',
      error:
        'This daemon token is missing. Edit the connection to add it again.',
    });

    return false;
  }

  const next = new WakuClient({
    address: profile.address,
    token,
    randomUUID,
    webSocketFactory: daemonSocketFactory,
  });

  const link = new DaemonLink({
    client: next,
    active: appInForeground(),
  });

  linkRef = link;
  daemonState.update((prev) => ({ ...prev, client: next }));
  let seenConnections = 0;
  unsubscribeLink = link.subscribe((snapshot) => {
    if (
      generation !== attempt ||
      linkRef !== link ||
      snapshot.phase === 'closed'
    ) {
      return;
    }

    setStatus({
      phase: snapshot.phase,
      error: snapshot.error,
      outage: snapshot.outage,
      connections: snapshot.connections,
    });

    if (snapshot.connections > seenConnections) {
      seenConnections = snapshot.connections;
      noteConnected(profile.id, snapshot.connections);
    }
  });

  unsubscribeTaskState = next.subscribeTaskState(() => {
    // Boss document mutations broadcast through the same notifier, so the
    // roster view re-fetches with the task list.
    invalidateDaemonQueries(profile.id, (kind) => kind === 'task-state' || kind === 'boss-state');
  });

  // The broadcast carries the whole settings document, so it lands in
  // the cache directly — no refetch for another client's edit or an
  // agent's settings write.
  unsubscribeSettings = next.subscribeSettings((settings) => {
    setDaemonQueryData(daemonKeys.settings(profile.id), normalizeDaemonSettings(settings));
  });

  unsubscribeBossSecondary = bindBossSecondary(next, profile.id);
  const connected = await link.open();
  return generation === attempt && linkRef === link && connected;
}

export async function reconnect(): Promise<boolean> {
  const id = activeProfileIdRef;
  if (!id) {return false;}
  const link = linkRef;
  if (!link) {return activate(id);}
  switch (link.state.phase) {
    case 'connected':
      return true;
    case 'connecting':
    case 'reconnecting':
      return link.retryNow();
    default:
      // A hard failure gets a fresh client with whatever token is stored now.
      return activate(id);
  }
}

export async function saveProfile(
  input: DaemonProfileInput,
  id?: string,
): Promise<{ profile: DaemonProfile; connected: boolean }> {
  const existing = id
    ? profilesRef.find((item) => item.id === id)
    : undefined;

  if (id && !existing)
    {throw new Error('This saved daemon no longer exists');}

  const profile = normalizeDaemonProfile(
    input,
    existing,
    id ?? randomUUID(),
  );

  if (
    profile.address.startsWith('ws://') &&
    !isPrivateDaemonAddress(profile.address)
  ) {
    throw new Error(
      'Use wss:// for a daemon outside your local or private network',
    );
  }

  const duplicate = profilesRef.find(
    (item) => item.id !== profile.id && item.address === profile.address,
  );

  if (duplicate)
    {throw new Error(`${duplicate.name} already uses this daemon address`);}

  const token = input.token?.trim();
  if (!existing && !token) {throw new Error('Enter the daemon token');}
  if (token) {await writeDaemonToken(profile.id, token);}
  else if (!(await readDaemonToken(profile.id)))
    {throw new Error('Enter the daemon token');}

  const next = existing
    ? profilesRef.map((item) =>
        item.id === profile.id ? profile : item,
      )
    : [profile, ...profilesRef];

  await commitProfiles(next);
  const connected = await activate(profile.id, next);
  return { profile, connected };
}

export function selectProfile(id: string): Promise<boolean> {
  return activate(id);
}

export async function removeProfile(id: string): Promise<void> {
  const current = profilesRef;
  if (!current.some((item) => item.id === id)) {return;}
  const next = current.filter((item) => item.id !== id);
  const token = await readDaemonToken(id);
  await deleteDaemonToken(id);
  try {
    await commitProfiles(next);
  } catch (cause) {
    if (token) {await writeDaemonToken(id, token);}
    throw cause;
  }

  removeDaemonQueries(id);
  if (activeProfileIdRef !== id) {return;}

  ++generation;
  closeCurrentLink();
  activeProfileIdRef = null;
  daemonState.update((prev) => ({ ...prev, activeProfile: null }));
  await writeActiveDaemonId(null);
  setStatus(IDLE);
  if (next.length) {await activate(next[0]!.id, next);}
}

export function disconnect() {
  ++generation;
  closeCurrentLink();
  setStatus(IDLE);
}

/** Kick the bootstrap from the app root — safe to call from every root
 * mount; runs once per process. */
export function bootstrapDaemon() {
  if (bootstrapped) {return;}
  bootstrapped = true;
  void hydratePersistentStorage();
  void (async () => {
    try {
      const [savedProfiles, savedActiveId] = await Promise.all([
        readDaemonProfiles(),
        readActiveDaemonId(),
      ]);

      profilesRef = savedProfiles;
      daemonState.update((prev) => ({ ...prev, profiles: savedProfiles }));
      if (!savedProfiles.length) {
        setStatus(IDLE);
        return;
      }

      const selected = savedProfiles.some((item) => item.id === savedActiveId)
        ? savedActiveId!
        : savedProfiles[0]!.id;

      await activate(selected, savedProfiles);
    } catch (cause) {
      setStatus({
        ...IDLE,
        phase: 'error',
        error: errorMessage(cause, 'Couldn’t load saved daemons'),
      });
    }
  })();
}

let lastForeground = true;
function appInForeground(): boolean {
  return lastForeground;
}

/** The link retries and probes only while the app is in the foreground;
 * returning to it triggers an immediate retry or a liveness probe. */
subscribeAppForeground((foreground) => {
  lastForeground = foreground;
  linkRef?.setActive(foreground);
});

function errorMessage(cause: unknown, fallback: string): string {
  if (cause instanceof Error && cause.message.trim()) {return cause.message;}
  return fallback;
}
