import type {
  AgentSession,
  BossEmployee,
  BossState,
  Project,
} from '@waku/client';

import {
  bossEmployeeLifecycle,
  bossPlanGroups,
  type BossPlanGroup,
} from '@waku/client/boss';

import { bossQueueRanks } from './boss';
import { relativeSessionTime } from './session-presentation';

/** Finished goals shown before the rest fold behind "Show more" — the
 * desktop's `GOALS_PANEL_RECENT_LIMIT`. */
export const GOALS_RECENT_LIMIT = 5;

/** The coarse execution bucket that decides which Goals section a row
 * renders under — mirrors the desktop's `BossGoalBucket`. */
export type GoalBucket = 'finished' | 'inProgress' | 'pending';

/** The specific status a goal row reports. Mirrors the desktop's
 * `BossGoalStatus`; glyphs and tones mirror the session status language. */
export type GoalStatus =
  | 'queued'
  | 'starting'
  | 'working'
  | 'backgroundWork'
  | 'needsInput'
  | 'paused'
  | 'usageLimited'
  | 'blocked'
  | 'attention'
  | 'finishing'
  | 'active'
  | 'unavailable'
  | 'failed'
  | 'budgetReached'
  | 'complete'
  | 'finished';

export const GOAL_STATUS_LABELS: Record<GoalStatus, string> = {
  queued: 'Queued',
  starting: 'Starting',
  working: 'Working',
  backgroundWork: 'Background work',
  needsInput: 'Needs input',
  paused: 'Paused',
  usageLimited: 'Usage limited',
  blocked: 'Needs attention',
  attention: 'Attention requested',
  finishing: 'Finishing',
  active: 'In progress',
  unavailable: 'Status unavailable',
  failed: 'Failed',
  budgetReached: 'Budget reached',
  complete: 'Completed',
  finished: 'Finished',
};

export type GoalTone = 'accent' | 'secondary' | 'tertiary' | 'warning' | 'danger' | 'success';

export interface GoalMarker {
  /** SF Symbol name; android/web leaves take material stand-ins. */
  symbol: string;
  tone: GoalTone;
  /** Starting and working markers spin like the sidebar's task rows. */
  spin: boolean;
}

/** Icon, tone, and whether it spins — mirrors `BossGoalStatus::marker`. */
export function goalStatusMarker(status: GoalStatus): GoalMarker {
  switch (status) {
    case 'starting':
    case 'working':
      return { symbol: 'arrow.triangle.2.circlepath', tone: 'accent', spin: true };
    case 'finishing':
      return { symbol: 'arrow.triangle.2.circlepath', tone: 'secondary', spin: true };
    case 'queued':
    case 'active':
    case 'backgroundWork':
      return { symbol: 'hourglass', tone: 'secondary', spin: false };
    case 'unavailable':
      return { symbol: 'hourglass', tone: 'tertiary', spin: false };
    case 'needsInput':
    case 'usageLimited':
    case 'blocked':
    case 'attention':
    case 'budgetReached':
      return { symbol: 'exclamationmark.triangle', tone: 'warning', spin: false };
    case 'paused':
      return { symbol: 'pause', tone: 'secondary', spin: false };
    case 'failed':
      return { symbol: 'xmark', tone: 'danger', spin: false };
    case 'complete':
      return { symbol: 'checkmark', tone: 'success', spin: false };
    case 'finished':
      return { symbol: 'checkmark', tone: 'secondary', spin: false };
  }
}

/** Whether the status counts toward a section's "needs attention" tally —
 * mirrors `BossGoalStatus::attention`. */
export function goalStatusNeedsAttention(status: GoalStatus): boolean {
  return (
    status === 'failed'
    || status === 'blocked'
    || status === 'attention'
    || status === 'needsInput'
    || status === 'paused'
    || status === 'usageLimited'
    || status === 'budgetReached'
  );
}

export interface GoalStatusResolution {
  bucket: GoalBucket;
  status: GoalStatus;
}

/** Lifecycle plus cached session evidence → section bucket and status, in
 * the design's precedence order: queue admission, explicit terminal
 * results, then live session signals. Mirrors the desktop's
 * `boss_goal_status`. */
export function bossGoalStatus(
  employee: BossEmployee,
  session: AgentSession | undefined,
): GoalStatusResolution {
  const lifecycle = bossEmployeeLifecycle(employee);
  const threadGoal = session?.thread_goal?.status;

  if (lifecycle === 'queued') {return { bucket: 'pending', status: 'queued' };}
  if (lifecycle === 'dispatching') {return { bucket: 'inProgress', status: 'starting' };}

  if (lifecycle === 'expired') {
    const status: GoalStatus = session?.status === 'failed'
      ? 'failed'
      : employee.blocker != null
        ? 'attention'
        : threadGoal === 'complete'
          ? 'complete'
          : threadGoal === 'budgetLimited'
            ? 'budgetReached'
            : 'finished';

    return { bucket: 'finished', status };
  }

  // working | finishing — terminal thread-goal results can outrank an
  // in-flight lifecycle while the record settles.
  if (session?.status === 'failed') {return { bucket: 'inProgress', status: 'failed' };}
  if (threadGoal === 'complete') {return { bucket: 'finished', status: 'complete' };}
  if (threadGoal === 'budgetLimited') {return { bucket: 'finished', status: 'budgetReached' };}
  if (lifecycle === 'finishing') {return { bucket: 'inProgress', status: 'finishing' };}
  if (employee.blocker != null) {return { bucket: 'inProgress', status: 'attention' };}

  const status: GoalStatus = threadGoal === 'blocked'
    ? 'blocked'
    : threadGoal === 'paused'
      ? 'paused'
      : threadGoal === 'usageLimited'
        ? 'usageLimited'
        : session?.status === 'waiting'
          ? 'needsInput'
          : session?.status === 'connecting'
            ? 'starting'
            : session?.status === 'working'
              ? 'working'
              : session?.status === 'background'
                ? 'backgroundWork'
                // An unfinished pursuit with no live turn — including an
                // admitted employee idle between prompts — reads active.
                : session
                  ? 'active'
                  : 'unavailable';

  return { bucket: 'inProgress', status };
}

/** The queued ticket's first nonempty prompt paragraph — present only
 * while the assignment waits; dispatch clears the ticket's prompt. */
function queuedObjective(employee: BossEmployee): string | null {
  const brief = employee.ticket?.prompt;
  if (!brief) {return null;}
  for (const paragraph of brief.split('\n\n')) {
    const trimmed = paragraph.trim();
    if (trimmed) {return trimmed;}
  }

  return null;
}

/** The row's resolved headline: a meaningful task title wins; the seeded
 * employee-name title must not suppress the objective fallbacks. Order
 * mirrors `prepare_boss_goal_row`: explicit → auto → goal objective →
 * first prompt paragraph → job title → "Untitled goal". */
export function goalTitle(
  employee: BossEmployee,
  session: AgentSession | undefined,
): string {
  const name = employee.identity.name;
  const explicit = session?.title.trim();
  if (explicit && explicit !== 'New task' && explicit !== name) {return explicit;}
  const auto = session?.auto_title?.trim();
  if (auto && auto !== name) {return auto;}
  const objective = session?.thread_goal?.objective.trim();
  if (objective) {return objective;}
  const queued = queuedObjective(employee);
  if (queued) {return queued;}
  return employee.jobTitle.trim() || 'Untitled goal';
}

/** The detail line's project part: the session's project name, else the
 * summon ticket's declared project while its session shell is absent. */
export function goalProjectLabel(
  employee: BossEmployee,
  session: AgentSession | undefined,
  projects: Map<string, Project>,
): string {
  const project = session ? projects.get(session.project_id) : undefined;
  const queued = employee.ticket?.project.trim();
  return project?.name ?? (queued || undefined) ?? 'Project unavailable';
}

/** Whether the session runs in a materialized worktree — a queued
 * worktree request has no fork marker until the checkout exists. */
function goalWorktree(session: AgentSession | undefined): boolean {
  return session?.workspace?.kind === 'worktree';
}

/** Last-updated: the task's own stamp, lifted by a newer admission or
 * finish timestamp when the session snapshot lags. */
export function goalUpdatedAt(
  employee: BossEmployee,
  session: AgentSession | undefined,
): number | null {
  const stamps = [
    session?.updated_at,
    employee.expiredAt,
    employee.queuedAt,
    employee.createdAt,
  ].filter((stamp): stamp is number => stamp != null);

  return stamps.length ? Math.max(...stamps) : null;
}

/** The detail line's trailing relative time — `"4m ago"`, `just now` for
 * the first minute, matching the desktop's `boss.goals_updated_ago`. */
export function goalUpdatedLabel(updatedAt: number | null, now: number): string | null {
  if (updatedAt == null) {return null;}
  const label = relativeSessionTime(updatedAt, now);
  return label === 'just now' ? label : `${label} ago`;
}

/** A goal row resolved against the cached session snapshot — everything
 * the list renders, prepared once per sheet refresh. */
export interface GoalRow {
  employee: BossEmployee;
  bucket: GoalBucket;
  status: GoalStatus;
  title: string;
  projectLabel: string;
  worktree: boolean;
  /** `blocker` or an attention status — leads In-progress sorting and
   * counts toward collapsed headers' tally. */
  attention: boolean;
  /** True while a session shell exists to navigate to. */
  navigable: boolean;
  updatedAt: number | null;
  updatedSort: number;
  createdSort: number;
  rankSort: number;
}

export function prepareGoalRow(
  employee: BossEmployee,
  session: AgentSession | undefined,
  projects: Map<string, Project>,
  queueRank: number | undefined,
): GoalRow {
  const { bucket, status } = bossGoalStatus(employee, session);
  const updatedAt = goalUpdatedAt(employee, session);
  return {
    employee,
    bucket,
    status,
    title: goalTitle(employee, session),
    projectLabel: goalProjectLabel(employee, session, projects),
    worktree: goalWorktree(session),
    attention: employee.blocker != null || goalStatusNeedsAttention(status),
    navigable: session != null,
    updatedAt,
    updatedSort: updatedAt ?? 0,
    createdSort: employee.createdAt ?? employee.queuedAt ?? 0,
    rankSort: queueRank ?? Number.MAX_SAFE_INTEGER,
  };
}

export interface GoalSections {
  finished: GoalRow[];
  inProgress: GoalRow[];
  pending: GoalRow[];
}

const bySessionId = (a: GoalRow, b: GoalRow) =>
  a.employee.sessionId < b.employee.sessionId
    ? -1
    : a.employee.sessionId > b.employee.sessionId
      ? 1
      : 0;

/** In-progress order: attention rows first, then oldest assignment —
 * mirrors the desktop's `running` sort. */
export function sortGoalRowsInProgress(rows: GoalRow[]): GoalRow[] {
  return rows.sort(
    (a, b) =>
      Number(b.attention) - Number(a.attention)
      || a.createdSort - b.createdSort
      || bySessionId(a, b),
  );
}

/** The Goals surface's section lists. Only `workGoal === 'goal'` records
 * reach them — errands never appear. `exclude` drops the sessionIds a
 * caller already rendered inside plan groups. Finished is newest first;
 * In progress leads with attention rows; Pending follows queue order. */
export function bossGoalSections(
  state: BossState,
  sessions: Map<string, AgentSession>,
  projects: Map<string, Project>,
  exclude?: ReadonlySet<string>,
): GoalSections {
  const ranks = bossQueueRanks(state);
  const sections: GoalSections = { finished: [], inProgress: [], pending: [] };
  for (const employee of state.employees) {
    if (employee.workGoal !== 'goal') {continue;}
    if (exclude?.has(employee.sessionId)) {continue;}
    const row = prepareGoalRow(
      employee,
      sessions.get(employee.sessionId),
      projects,
      ranks.get(employee.sessionId),
    );

    sections[row.bucket].push(row);
  }

  // Finished is newest first; In progress leads with actionable items then
  // assignment age; Pending follows daemon admission order.
  sections.finished.sort(
    (a, b) => b.updatedSort - a.updatedSort || bySessionId(a, b),
  );

  sortGoalRowsInProgress(sections.inProgress);
  sections.pending.sort(
    (a, b) => a.rankSort - b.rankSort || bySessionId(a, b),
  );

  return sections;
}

export interface GoalPlanGroupItem {
  itemId: string;
  title: string;
  progress: 'toDo' | 'inProgress' | 'done' | 'dropped';
  rows: GoalRow[];
}

/** A plan group prepared for the sheet: its items in declared order with
 * each item's linked goal employees as rows, then the tagged employees
 * with no live item. Members keep the surface's in-progress ordering —
 * attention first, then oldest. */
export interface GoalPlanGroup {
  planId: string;
  sessionId: string;
  title: string;
  finalized: boolean;
  items: GoalPlanGroupItem[];
  unallocated: GoalRow[];
  lastActivityAt: number;
  /** Rows needing attention across items and the unallocated tail. */
  attention: number;
  /** Session ids the group absorbs — the flat sections skip them. */
  memberIds: Set<string>;
}

function goalRowFor(
  employee: BossEmployee,
  sessions: Map<string, AgentSession>,
  projects: Map<string, Project>,
  ranks: Map<string, number>,
): GoalRow {
  return prepareGoalRow(employee, sessions.get(employee.sessionId), projects, ranks.get(employee.sessionId));
}

/** Plan groups for the ongoing area, with linked employees prepared as
 * goal rows. Only `workGoal === 'goal'` members render — the daemon's
 * grouping is work-kind agnostic but errands never appear on this
 * surface. Mirrors the ordering `BossState::plan_groups` exposes. */
export function bossGoalPlanGroups(
  state: BossState,
  sessions: Map<string, AgentSession>,
  projects: Map<string, Project>,
): GoalPlanGroup[] {
  const ranks = bossQueueRanks(state);
  return bossPlanGroups(state).map((group: BossPlanGroup) => {
    const items = group.items.map((entry) => ({
      itemId: entry.item.id,
      title: entry.item.title,
      progress: entry.progress,
      rows: sortGoalRowsInProgress(
        entry.employees
          .filter((employee) => employee.workGoal === 'goal')
          .map((employee) => goalRowFor(employee, sessions, projects, ranks)),
      ),
    }));

    const unallocated = sortGoalRowsInProgress(
      group.unallocated
        .filter((employee) => employee.workGoal === 'goal')
        .map((employee) => goalRowFor(employee, sessions, projects, ranks)),
    );

    const all = [...items.flatMap((entry) => entry.rows), ...unallocated];
    return {
      planId: group.plan.id,
      sessionId: group.plan.sessionId,
      title: group.plan.idea.trim() || 'Plan',
      finalized: group.plan.finalizedAt != null,
      items,
      unallocated,
      lastActivityAt: group.lastActivityAt,
      attention: all.filter((row) => row.attention).length,
      memberIds: new Set(
        group.items
          .flatMap((entry) => entry.employees)
          .concat(group.unallocated)
          .filter((employee) => employee.workGoal === 'goal')
          .map((employee) => employee.sessionId),
      ),
    };
  });
}

/** Finished goals the user has not opened the surface since — the
 * `goalsViewedAt` unread contract the daemon stores for the sidebar
 * badge. A missing stamp means the surface was never viewed, so every
 * finished goal reads as unread, matching the deliverable rule. */
export function bossGoalsUnreadCount(state: BossState): number {
  const viewed = state.goalsViewedAt ?? 0;
  return state.employees.filter(
    (employee) =>
      employee.workGoal === 'goal'
      && employee.expiredAt != null
      && employee.expiredAt > viewed,
  ).length;
}
