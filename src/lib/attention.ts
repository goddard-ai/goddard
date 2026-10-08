import type {
  AgentSession,
  BossDeliverable,
  BossEmployee,
  BossPlan,
  BossState,
} from '@waku/client';

import {
  bossAttention,
  bossEmployeeLifecycle,
  deliverableUnread,
} from '@waku/client/boss';

/** A session's device-local reply watermark — `UnseenReplyWatermarks` from
 * `unseen-replies`, re-declared here so this module stays free of the
 * storage stack for bun tests. */
export type SessionWatermarks = Record<string, number>;

/** How long a "next attention" chain keeps its visited set after the last
 * jump — the desktop's `UNREAD_SWEEP_TIMEOUT`. Past it, a press sweeps
 * fresh instead of skipping rows the chain already showed. */
export const ATTENTION_SWEEP_TIMEOUT_SECS = 10 * 60;

/** The kinds the boss-attention surface reports. Decision kinds block on
 * the user; the rest are arrivals worth one look. Ordinary tasks join the
 * sweep through `waiting`/`unseenReply` — the desktop's unread sweep folds
 * both in — but they never appear in the boss chat's strip. */
export type AttentionItemKind =
  | 'blocker'
  | 'bossQuestion'
  | 'planApproval'
  | 'employeeWaiting'
  | 'waiting'
  | 'deliverable'
  | 'goalFinished'
  | 'planFinished'
  | 'unseenReply';

/** What a card tap or the sweep resolves to. A deliverable lands on its
 * boss chat with the record armed — the desktop's `UnreadTarget`
 * equivalent — and falls back to the library when no boss chat exists. */
export type AttentionTarget =
  | { type: 'session'; sessionId: string }
  | { type: 'deliverable'; deliverableId: string };

export interface AttentionItem {
  /** Stable identity — dedup, sweep visited set, notification tag. */
  key: string;
  kind: AttentionItemKind;
  /** Sort bucket — lower leads the strip and the sweep. */
  rank: number;
  title: string;
  detail: string | null;
  target: AttentionTarget;
  /** unix seconds, when the underlying record carries a stamp. */
  at?: number;
}

const KIND_RANK: Record<AttentionItemKind, number> = {
  blocker: 0,
  bossQuestion: 10,
  planApproval: 20,
  employeeWaiting: 30,
  waiting: 40,
  deliverable: 50,
  goalFinished: 60,
  planFinished: 60,
  unseenReply: 70,
};

/** Kinds that need a decision — the boss row's "needs attention" count and
 * the strip's warning cards. */
const DECISION_KINDS: ReadonlySet<AttentionItemKind> = new Set([
  'blocker',
  'bossQuestion',
  'planApproval',
  'employeeWaiting',
  'waiting',
]);

/** Mirrors `sessionHasUnseenReply` in `unseen-replies` — a session reads as
 * unread once its reply stamp advances past the watermark this device last
 * had on screen. Untracked sessions (no watermark) are not unread. */
function sessionUnread(
  session: AgentSession,
  watermarks: SessionWatermarks,
): boolean {
  const seen = watermarks[session.id];
  return (
    seen !== undefined &&
    session.last_reply_at != null &&
    session.last_reply_at > seen
  );
}

export interface AttentionContext {
  /** The daemon's Boss document — `null` when the boss experiment is off. */
  state: BossState | null;
  /** The daemon's session list projections. */
  sessions: AgentSession[];
  /** This device's per-session reply watermarks. */
  watermarks: SessionWatermarks;
}

/** The "needs you" inventory for one daemon — the mobile pull set plus the
 * session-level waits and unseen replies the desktop's sweep already
 * covers. Every item is derived from a durable signal (blocker, status,
 * `viewedAt`, `goalsViewedAt`, watermarks), so a dismissed or resolved
 * item leaves the list on its own — no arithmetic that nags. Ordering:
 * decisions first (blockers, asks), arrivals next (deliverables, finishes),
 * plain unread replies last; boss chat's own unread marker trails —
 * a boss reply never outranks real work waiting, the desktop's rule. */
export function collectAttentionItems(context: AttentionContext): AttentionItem[] {
  const items: AttentionItem[] = [];
  const { state, sessions, watermarks } = context;
  const byId = new Map(sessions.map((session) => [session.id, session]));
  const bossSessionId = state?.sessionId ?? null;
  const employees = new Map<string, BossEmployee>();
  if (state) {
    for (const employee of [...state.employees, ...state.retiredEmployees]) {
      employees.set(employee.sessionId, employee);
    }
  }

  const planningSessions = new Map<string, BossPlan>();
  for (const plan of state?.planning ?? []) {
    planningSessions.set(plan.sessionId, plan);
  }

  // Decisions: durable blockers first, then parked asks.
  for (const employee of state?.employees ?? []) {
    if (employee.blocker == null) {continue;}
    items.push({
      key: `blocker:${employee.sessionId}`,
      kind: 'blocker',
      rank: KIND_RANK.blocker,
      title: `${employee.identity.name} needs attention`,
      detail: employee.blocker,
      target: { type: 'session', sessionId: employee.sessionId },
      at: employee.expiredAt ?? employee.createdAt ?? undefined,
    });
  }

  const bossSession = bossSessionId ? byId.get(bossSessionId) : undefined;
  if (bossSession?.status === 'waiting' && state) {
    items.push({
      key: `wait:${bossSession.id}`,
      kind: 'bossQuestion',
      rank: KIND_RANK.bossQuestion,
      title: `${state.identity.name} is waiting for an answer`,
      detail: 'Waiting for your input',
      target: { type: 'session', sessionId: bossSession.id },
      at: bossSession.updated_at,
    });
  }

  for (const plan of state?.planning ?? []) {
    const session = byId.get(plan.sessionId);
    if (plan.finalizedAt != null || session?.status !== 'waiting') {continue;}
    items.push({
      key: `wait:${plan.sessionId}`,
      kind: 'planApproval',
      rank: KIND_RANK.planApproval,
      title: 'Plan ready for approval',
      detail: plan.idea.trim() || null,
      target: { type: 'session', sessionId: plan.sessionId },
      at: session.updated_at,
    });
  }

  for (const session of sessions) {
    if (session.status !== 'waiting') {continue;}
    if (session.id === bossSessionId || planningSessions.has(session.id)) {continue;}
    const employee = employees.get(session.id);
    items.push({
      key: `wait:${session.id}`,
      kind: employee ? 'employeeWaiting' : 'waiting',
      rank: employee ? KIND_RANK.employeeWaiting : KIND_RANK.waiting,
      title: employee
        ? `${employee.identity.name} is waiting for you`
        : `${sessionTitle(session)} is waiting for you`,
      detail: employee?.jobTitle ?? null,
      target: { type: 'session', sessionId: session.id },
      at: session.updated_at,
    });
  }

  // Arrivals: unread deliverables, then finishes newer than the goals stamp.
  const goalsViewedAt = state?.goalsViewedAt ?? 0;
  for (const deliverable of state?.deliverables ?? []) {
    if (!deliverableUnread(deliverable)) {continue;}
    items.push({
      key: `deliverable:${deliverable.id}`,
      kind: 'deliverable',
      rank: KIND_RANK.deliverable,
      title: deliverable.name,
      detail: 'New deliverable',
      target: { type: 'deliverable', deliverableId: deliverable.id },
      at: deliverable.updatedAt,
    });
  }

  for (const employee of state?.employees ?? []) {
    if (
      employee.workGoal !== 'goal'
      || employee.expiredAt == null
      || employee.cancelled
      || employee.blocker != null
      || employee.expiredAt <= goalsViewedAt
    ) {continue;}

    items.push({
      key: `goalFinished:${employee.sessionId}`,
      kind: 'goalFinished',
      rank: KIND_RANK.goalFinished,
      title: `${employee.identity.name} finished`,
      detail: employee.expiry?.pendingQuestion ?? employee.jobTitle,
      target: { type: 'session', sessionId: employee.sessionId },
      at: employee.expiredAt,
    });
  }

  for (const plan of state?.planning ?? []) {
    if (plan.outcome !== 'completed' && plan.outcome !== 'abandoned') {continue;}
    const at = plan.history?.at(-1)?.at ?? plan.finalizedAt ?? 0;
    if (at <= goalsViewedAt) {continue;}
    items.push({
      key: `planFinished:${plan.id}`,
      kind: 'planFinished',
      rank: KIND_RANK.planFinished,
      title: `Plan ${plan.outcome === 'completed' ? 'completed' : 'abandoned'}`,
      detail: plan.idea.trim() || null,
      target: { type: 'session', sessionId: plan.sessionId },
      at,
    });
  }

  // Unread replies scan last — ordinary sessions first, the boss chat last,
  // matching the desktop sweep's "boss rows answer after real work".
  for (const session of sessions) {
    if (session.id === bossSessionId || !sessionUnread(session, watermarks)) {continue;}
    items.push(unseenItem(session, employees.get(session.id), false));
  }

  if (bossSession && sessionUnread(bossSession, watermarks)) {
    items.push(unseenItem(bossSession, undefined, true));
  }

  return items.sort((a, b) => a.rank - b.rank || (b.at ?? 0) - (a.at ?? 0) || a.key.localeCompare(b.key));
}

function unseenItem(
  session: AgentSession,
  employee: BossEmployee | undefined,
  bossChat: boolean,
): AttentionItem {
  return {
    key: `unseen:${session.id}`,
    kind: 'unseenReply',
    rank: KIND_RANK.unseenReply + (bossChat ? 1 : 0),
    title: employee ? employee.identity.name : sessionTitle(session),
    detail: employee?.jobTitle ?? 'New reply',
    target: { type: 'session', sessionId: session.id },
    at: session.last_reply_at ?? undefined,
  };
}

function sessionTitle(session: AgentSession): string {
  const title = (session.auto_title || session.title).trim();
  return title === 'New task' || !title ? 'A task' : title;
}

/** `kind`s that belong on the boss chat's attention strip — the boss-owned
 * pull set. Ordinary tasks' waits and unreads stay out: the strip is the
 * aggregate boss view, not a second task list. The boss chat's own pending
 * ask renders above the composer already, so it never double-mounts as a
 * card. */
export function stripItems(items: AttentionItem[]): AttentionItem[] {
  return items.filter(
    (item) =>
      item.kind !== 'waiting'
      && item.kind !== 'unseenReply'
      && item.kind !== 'bossQuestion',
  );
}

export interface AttentionCounts {
  /** Items blocked on a decision. */
  needsYou: number;
  /** Arrivals worth a look but no decision. */
  fresh: number;
}

export function attentionCounts(items: AttentionItem[]): AttentionCounts {
  let needsYou = 0;
  let fresh = 0;
  for (const item of items) {
    if (DECISION_KINDS.has(item.kind)) {needsYou += 1;}
    else {fresh += 1;}
  }

  return { needsYou, fresh };
}

/** The boss-row subtitle's two counters — decisions, then arrivals that
 * have their own seen contract (deliverables, goal/plan finishes). Plain
 * unseen replies carry the existing trailing dot instead. */
export function bossAttentionCounts(items: AttentionItem[]): AttentionCounts {
  return attentionCounts(items.filter((item) => item.kind !== 'unseenReply'));
}

/** The sweep's next landing: the first item the chain hasn't already
 * shown, skipping the session on screen. Mirrors the desktop's
 * `next_unread_completion` — landing clears each target's own marker, and
 * `visited` keeps presses advancing past items whose marker only clears
 * elsewhere (a blocker survives visiting the employee's session). */
export function nextAttentionItem(
  items: AttentionItem[],
  visited: ReadonlySet<string>,
  excludeSessionId?: string | null,
): AttentionItem | null {
  for (const item of items) {
    if (visited.has(item.key)) {continue;}
    if (
      excludeSessionId != null
      && item.target.type === 'session'
      && item.target.sessionId === excludeSessionId
    ) {continue;}

    return item;
  }

  return null;
}

export type AttentionEvent =
  | { type: 'employeeBlocker'; employee: BossEmployee }
  | { type: 'deliverablePublished'; deliverable: BossDeliverable }
  | { type: 'deliverableUpdated'; deliverable: BossDeliverable }
  | { type: 'planFinalized'; plan: BossPlan }
  | { type: 'employeeFinished'; employee: BossEmployee }
  | { type: 'planFinished'; plan: BossPlan };

/** Boss-document transitions worth a notification — the client's
 * `bossAttention` diff plus the expiry and plan-outcome transitions it
 * doesn't cover ("goal/plan finishes"). Only transitions report: the first
 * sync (`previous` absent) and identity changes report nothing, so a
 * reconnect's catch-up fires exactly the events missed while offline.
 * Cancelled jobs and finishes already carrying a blocker stay quiet — the
 * blocker report was the notification. */
export function bossAttentionEvents(
  previous: BossState | null | undefined,
  current: BossState,
): AttentionEvent[] {
  if (!previous || previous.identity.id !== current.identity.id) {return [];}

  const events: AttentionEvent[] = bossAttention(previous, current);
  for (const employee of current.employees) {
    const before = previous.employees.find(
      (entry) => entry.sessionId === employee.sessionId,
    );

    if (!before) {continue;}
    const finished =
      bossEmployeeLifecycle(before) !== 'expired'
      && bossEmployeeLifecycle(employee) === 'expired';

    if (
      finished
      && !employee.cancelled
      && employee.blocker == null
      && employee.workGoal === 'goal'
    ) {
      events.push({ type: 'employeeFinished', employee });
    }
  }

  for (const plan of current.planning) {
    const before = previous.planning.find((entry) => entry.id === plan.id);
    if (!before) {continue;}
    const finished =
      (plan.outcome === 'completed' || plan.outcome === 'abandoned')
      && before.outcome !== plan.outcome;

    if (finished) {events.push({ type: 'planFinished', plan });}
  }

  return events;
}

/** Sessions that entered `waiting` since the last list — a parked
 * permission or structured question. The feed stores the compact
 * id → status map rather than full projections; `undefined` (first
 * snapshot) reports nothing, same as `bossAttentionEvents`. */
export function waitingTransitions(
  previous: ReadonlyMap<string, AgentSession['status']> | undefined,
  current: AgentSession[],
): AgentSession[] {
  if (!previous) {return [];}
  return current.filter(
    (session) => session.status === 'waiting' && previous.get(session.id) !== 'waiting',
  );
}

export interface AttentionNoticeText {
  /** Notification dedup tag — stable per event instance. */
  key: string;
  title: string;
  body: string;
  target: AttentionTarget;
}

/** Notification copy for one boss-state transition. */
export function attentionEventText(event: AttentionEvent): AttentionNoticeText {
  switch (event.type) {
    case 'employeeBlocker':
      return {
        // The blocker text joins the tag: a resolved-and-reported-again
        // blocker on the same employee is a new event, not a refire.
        key: `blocker:${event.employee.sessionId}:${event.employee.blocker ?? ''}`,
        title: `${event.employee.identity.name} needs attention`,
        body: event.employee.blocker ?? 'Reported a blocker',
        target: { type: 'session', sessionId: event.employee.sessionId },
      };
    case 'deliverablePublished':
      return {
        key: `deliverable:${event.deliverable.id}@${event.deliverable.updatedAt}`,
        title: 'New deliverable',
        body: event.deliverable.name,
        target: { type: 'deliverable', deliverableId: event.deliverable.id },
      };
    case 'deliverableUpdated':
      return {
        key: `deliverable:${event.deliverable.id}@${event.deliverable.updatedAt}`,
        title: 'Deliverable updated',
        body: event.deliverable.name,
        target: { type: 'deliverable', deliverableId: event.deliverable.id },
      };
    case 'planFinalized':
      return {
        key: `planFinalized:${event.plan.id}`,
        title: 'Plan finalized',
        body: event.plan.idea.trim() || event.plan.planFile,
        target: { type: 'session', sessionId: event.plan.sessionId },
      };
    case 'employeeFinished':
      return {
        key: `goalFinished:${event.employee.sessionId}@${event.employee.expiredAt ?? 0}`,
        title: `${event.employee.identity.name} finished`,
        body: event.employee.jobTitle,
        target: { type: 'session', sessionId: event.employee.sessionId },
      };
    case 'planFinished':
      return {
        key: `planFinished:${event.plan.id}`,
        title: `Plan ${event.plan.outcome === 'abandoned' ? 'abandoned' : 'completed'}`,
        body: event.plan.idea.trim() || event.plan.planFile,
        target: { type: 'session', sessionId: event.plan.sessionId },
      };
  }
}

/** Notification copy for a session newly parked on user input — the
 * boss's own asks name the boss, a planning session's parked card is the
 * plan approval, an employee's names the job. */
export function waitingNotice(
  session: AgentSession,
  state: BossState | null,
): AttentionNoticeText {
  const target: AttentionTarget = { type: 'session', sessionId: session.id };
  // updated_at rides the tag — an answered ask that parks again is a new
  // notification, the same transition still dedupes.
  const key = `wait:${session.id}@${session.updated_at}`;
  if (state && session.id === state.sessionId) {
    return {
      key,
      title: `${state.identity.name} is waiting for you`,
      body: 'Waiting for your input',
      target,
    };
  }

  const plan = state?.planning.find((entry) => entry.sessionId === session.id);
  if (plan) {
    return {
      key,
      title: 'Plan ready for approval',
      body: plan.idea.trim() || 'Review the finalized plan',
      target,
    };
  }

  const employee = [...(state?.employees ?? []), ...(state?.retiredEmployees ?? [])].find(
    (entry) => entry.sessionId === session.id,
  );

  if (employee) {
    return {
      key,
      title: `${employee.identity.name} is waiting for you`,
      body: employee.jobTitle,
      target,
    };
  }

  return {
    key,
    title: sessionTitle(session),
    body: 'Waiting for your input',
    target,
  };
}
