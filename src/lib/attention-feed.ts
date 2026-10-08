import type { AgentSession, BossState } from '@waku/client';

import { loadBossState, markDeliverableViewed } from '@waku/client/boss';

import { pushRoute } from '@octane-xplat/ui';

import { armDeliverable } from './deliverable-arm';
import {
  attentionEventText,
  bossAttentionEvents,
  waitingNotice,
  waitingTransitions,
  type AttentionTarget,
} from './attention';

import { subscribeAppForeground } from './app-foreground';
import { loadTaskState, daemonKeys, type TaskState } from './daemon-api';
import { daemonQueryEntry, getDaemonQueryData } from './daemon-data';
import { daemonState } from './daemon-store';
import {
  ensureLocalNotifications,
  onLocalNotificationTap,
  postLocalNotification,
  type LocalNoticeTarget,
} from './local-notifications';

/** Per-profile memory of the last delivered snapshots — transitions diff
 * against it, so a reconnect's refetch catches up on exactly the events
 * missed while the link was down. */
interface ProfileMemory {
  boss: BossState | null | undefined;
  statuses: Map<string, AgentSession['status']> | undefined;
}

const memories = new Map<string, ProfileMemory>();
/** Notification tags already delivered this process — the diff itself
 * dedups steady state, this guards against entry churn refiring one. */
const firedKeys = new Set<string>();

let installed = false;
let foreground = true;
let previousPhase: string | null = null;
/** True once a "connection lost" notice went out — the regain notice only
 * follows a loss this device reported, never a reconnect the user watched. */
let outageAnnounced = false;
let outageTimer: ReturnType<typeof setTimeout> | null = null;
let activeWatch: { profileId: string; unsubscribes: Array<() => void> } | null = null;

/** iOS suspends the app on backgrounding and the link drops for
 * lifecycle reasons — that isn't an outage. The loss banner only earns
 * itself if the link stays down past the defer window. On a suspended
 * process the timer freezes too, so the false case never fires. */
const OUTAGE_NOTICE_DEFER_MS = 8_000;

/** Installs the app-wide attention feed once — called from the daemon
 * bootstrap hook. Watching the query entries (not polling) means the same
 * `taskStateChanged` refetches that keep the drawer fresh also drive
 * backgrounded notifications: the daemon has no push channel for boss
 * attention, so the badge's freshness is the query's freshness. */
export function installAttentionFeed(): void {
  if (installed) {return;}
  installed = true;

  subscribeAppForeground((nowForeground) => {
    foreground = nowForeground;
  });

  onLocalNotificationTap(openAttentionTarget);
  daemonState.subscribe(syncDaemon);
  syncDaemon();
}

function syncDaemon() {
  const { phase, activeProfile, client, outage } = daemonState.get();
  const profileId = activeProfile?.id;

  // Connection lost/regained on the active daemon — the banner handles the
  // foreground case; this is the backgrounded OS banner.
  const name = activeProfile?.name ?? 'the daemon';
  const linkDown = phase === 'reconnecting' || phase === 'error';
  if (!foreground && previousPhase === 'connected' && linkDown && !outageTimer) {
    const since = outage?.since ?? Date.now();
    outageTimer = setTimeout(() => {
      outageTimer = null;
      const current = daemonState.get();
      if (foreground || (current.phase !== 'reconnecting' && current.phase !== 'error')) {return;}
      outageAnnounced = true;
      fire(`connection:lost@${since}`, {
        title: `Lost connection to ${current.activeProfile?.name ?? name}`,
        body: 'Retrying in the background',
        target: { kind: 'route', name: 'daemons' },
      });
    }, OUTAGE_NOTICE_DEFER_MS);
  }

  if (phase === 'connected') {
    if (outageTimer) {
      clearTimeout(outageTimer);
      outageTimer = null;
    }

    if (!foreground && previousPhase !== 'connected' && previousPhase !== null && outageAnnounced) {
      fire(`connection:regained@${Date.now()}`, {
        title: `Reconnected to ${name}`,
        body: 'The daemon link is back',
        target: { kind: 'route', name: 'index' },
      });
    }

    outageAnnounced = false;
  }

  previousPhase = phase;

  const watchProfile = phase === 'connected' && client && profileId ? profileId : null;
  if (activeWatch && activeWatch.profileId !== watchProfile) {
    for (const unsubscribe of activeWatch.unsubscribes) {unsubscribe();}
    activeWatch = null;
  }

  if (watchProfile && !activeWatch) {
    activeWatch = { profileId: watchProfile, unsubscribes: subscribeDaemon(watchProfile) };
  }
}

/** Subscribing makes the entries observed, so `taskStateChanged`
 * invalidations keep boss state fresh even while no boss surface is
 * mounted — that's what lets a badge appear the moment something happens
 * rather than when the user next looks. */
function subscribeDaemon(profileId: string): Array<() => void> {
  const memory = memories.get(profileId) ?? { boss: undefined, statuses: undefined };
  memories.set(profileId, memory);

  // The 1s stale time matches the hooks' — the feed may create the entry
  // first on cold start, and its options stick.
  const bossEntry = daemonQueryEntry<BossState | null>(
    daemonKeys.bossState(profileId),
    () => loadBossState(requireClient()),
    1_000,
  );

  const taskEntry = daemonQueryEntry<TaskState>(
    daemonKeys.taskState(profileId),
    () => loadTaskState(requireClient()),
    1_000,
  );

  return [
    bossEntry.subscribe(() => onBossState(memory, bossEntry.get().data)),
    taskEntry.subscribe(() => onTaskState(memory, taskEntry.get().data)),
  ];
}

function onBossState(memory: ProfileMemory, current: BossState | null | undefined) {
  const previous = memory.boss;
  memory.boss = current;
  if (!current || !previous || foreground) {return;}

  for (const event of bossAttentionEvents(previous, current)) {
    const notice = attentionEventText(event);
    fire(notice.key, {
      title: notice.title,
      body: notice.body,
      target: toNoticeTarget(notice.target),
    });
  }
}

function onTaskState(memory: ProfileMemory, current: TaskState | undefined) {
  const previousStatuses = memory.statuses;
  memory.statuses = current
    ? new Map(current.sessions.map((session) => [session.id, session.status]))
    : memory.statuses;

  if (!current || !previousStatuses || foreground) {return;}

  const state = memory.boss ?? null;
  for (const session of waitingTransitions(previousStatuses, current.sessions)) {
    const notice = waitingNotice(session, state);
    fire(notice.key, {
      title: notice.title,
      body: notice.body,
      target: toNoticeTarget(notice.target),
    });
  }
}

function fire(
  key: string,
  notice: { title: string; body: string; target?: LocalNoticeTarget },
) {
  if (firedKeys.has(key)) {return;}
  firedKeys.add(key);
  if (firedKeys.size > 500) {
    // Bounded memory: keep the newest half rather than grow without end.
    const entries = [...firedKeys];
    firedKeys.clear();
    for (const entry of entries.slice(-250)) {firedKeys.add(entry);}
  }

  postLocalNotification({ key, ...notice });
}

function toNoticeTarget(target: AttentionTarget): LocalNoticeTarget {
  return target.type === 'session'
    ? { kind: 'session', sessionId: target.sessionId }
    : { kind: 'deliverable', deliverableId: target.deliverableId };
}

/** A notification tap deep-links to the actionable surface, not the app
 * root: sessions open their task, a deliverable opens its boss chat with
 * the record armed (the library when the chat has never been opened), and
 * connection notices land where the link can be fixed. */
function openAttentionTarget(target: LocalNoticeTarget): void {
  if (target.kind === 'route') {
    pushRoute({ stack: 'root', name: target.name, params: {} });
    return;
  }

  if (target.kind === 'deliverable') {
    const { activeProfile, client } = daemonState.get();
    const state = activeProfile
      ? getDaemonQueryData<BossState | null>(daemonKeys.bossState(activeProfile.id))
      : undefined;

    const deliverable = state?.deliverables.find((item) => item.id === target.deliverableId);
    if (deliverable && client) {
      void markDeliverableViewed(client, deliverable.id).catch(() => {});
    }

    if (deliverable && state?.sessionId) {
      pushRoute({ stack: 'root', name: 'session/:id', params: { id: state.sessionId } });
      armDeliverable({ deliverable, sessionId: state.sessionId, previewOpen: true });
      return;
    }

    pushRoute({ stack: 'root', name: 'deliverables', params: {} });
    return;
  }

  pushRoute({ stack: 'root', name: 'session/:id', params: { id: target.sessionId } });
}

function requireClient() {
  const client = daemonState.get().client;
  if (!client) {throw new Error('Goddard daemon is disconnected');}
  return client;
}

export { ensureLocalNotifications };
