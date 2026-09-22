import type { AgentSession } from '@waku/client';

import {
  hydratePersistentStorage,
  persistentStorageSync,
} from './composer-preferences-store';

const STORAGE_KEY = 'waku.mobile.unseen-replies.v1';

/** sessionId → the newest `last_reply_at` the user has had on screen. The
 * daemon has no read state — like the desktop's `unseen_completions`, the
 * marker is per-device. */
export type UnseenReplyWatermarks = Record<string, number>;
type Store = Record<string, UnseenReplyWatermarks>;

const EMPTY: UnseenReplyWatermarks = {};

let cache: Store | null = null;
/** Writes serialize after storage hydration so a mark that lands before the
 * persisted map loads can neither lose it nor be lost to it. */
let writeQueue: Promise<void> = Promise.resolve();
const listeners = new Set<() => void>();

export function subscribeUnseenReplies(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** `useSyncExternalStore` snapshot: one daemon's watermark map. Maps are
 * replaced, never edited, so the reference is stable between mutations. */
export function unseenReplyWatermarks(
  daemonAddress: string | undefined,
): UnseenReplyWatermarks {
  if (!cache) loadSnapshot();
  return (daemonAddress ? cache?.[daemonAddress] : null) ?? EMPTY;
}

function loadSnapshot() {
  if (persistentStorageSync()) {
    cache = parseStore();
    return;
  }
  // Hydration is already in flight from the daemon bootstrap; when it lands
  // the real watermarks replace this empty snapshot.
  void hydratePersistentStorage().then(() => {
    if (!cache) {
      cache = parseStore();
      emit();
    }
  });
}

function parseStore(): Store {
  try {
    const parsed = JSON.parse(
      persistentStorageSync()?.getItem(STORAGE_KEY) ?? '{}',
    ) as unknown;
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as Store;
    }
  } catch {
    // Malformed disposable state; replace it.
  }
  return {};
}

function mutateStore(mutate: (store: Store) => Store | null): Promise<void> {
  writeQueue = writeQueue
    .then(async () => {
      await hydratePersistentStorage();
      cache ??= parseStore();
      const next = mutate(cache);
      if (!next || next === cache) return;
      cache = next;
      persistentStorageSync()?.setItem(STORAGE_KEY, JSON.stringify(next));
      emit();
    })
    .catch(() => {});
  return writeQueue;
}

/** A session reads as unread once its reply stamp advances past what the user
 * last had on screen. A missing watermark means untracked — the seed decides
 * whether its history counts as new. */
export function sessionHasUnseenReply(
  session: AgentSession,
  watermarks: UnseenReplyWatermarks,
): boolean {
  const seen = watermarks[session.id];
  return (
    seen !== undefined &&
    session.last_reply_at != null &&
    session.last_reply_at > seen
  );
}

/** Opening a session marks its newest reply seen. Called on every
 * `last_reply_at` change while it stays on screen, so a turn the user watches
 * finish never earns a dot — only activity after the visit does. */
export function markSessionRepliesSeen(
  daemonAddress: string,
  session: AgentSession,
): Promise<void> {
  return mutateStore((store) => {
    const replyAt = session.last_reply_at ?? 0;
    const watermarks = store[daemonAddress] ?? EMPTY;
    if ((watermarks[session.id] ?? 0) >= replyAt) return null;
    return {
      ...store,
      [daemonAddress]: { ...watermarks, [session.id]: replyAt },
    };
  });
}

/** Fills watermarks for sessions the map doesn't know yet. First contact with
 * a daemon treats existing history as read — an upgrade must not light up
 * every chat — while a session appearing later is new to this device and
 * seeds at 0, so replies it already carries (or the next one) flag it. */
export function seedUnseenReplyWatermarks(
  daemonAddress: string,
  sessions: AgentSession[],
): Promise<void> {
  return mutateStore((store) => {
    const watermarks = store[daemonAddress];
    const firstContact = watermarks === undefined;
    let next: UnseenReplyWatermarks | null = null;
    for (const session of sessions) {
      if (watermarks?.[session.id] !== undefined) continue;
      next ??= { ...watermarks };
      next[session.id] = firstContact ? (session.last_reply_at ?? 0) : 0;
    }
    return next && { ...store, [daemonAddress]: next };
  });
}

/** Test seam: drops the in-memory snapshot and pending writes so the next
 * read re-parses storage. */
export function resetUnseenReplyStore(): void {
  cache = null;
  writeQueue = Promise.resolve();
  listeners.clear();
}

function emit() {
  for (const listener of listeners) listener();
}
