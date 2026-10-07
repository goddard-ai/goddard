import type {
  AgentSession,
  AgentSessionTranscript,
  BossDeliverable,
  BossEmployee,
  BossFile,
  BossIdentity,
  BossOperation,
  BossPlan,
  BossResult,
  BossState,
  Command,
  EmployeeLifecycle,
  PlanItem,
  PlanItemState,
  PlanOutcome,
  Project,
  ProviderKind,
  ResponsePayload,
  RuntimeMode,
  WireDriverEvent,
} from './generated'
import type { EventListener, RequestOptions, WakuClient } from './client'

/** The `WakuClient` surface the boss helpers call — a paired client speaks
 * as the human (`sessionId`/`runtimeId` stay nil), so every boss operation
 * and read here resolves with the daemon's owner authority. */
export type BossClient = Pick<WakuClient, 'request' | 'subscribe'>

export function bossCommand(operation: BossOperation): Command {
  return { type: 'boss', operation }
}

/** Send one `boss` command and unwrap its result. Throws on a non-boss
 * payload the way the app's `expectResponse` does. */
export async function bossRequest(
  client: BossClient,
  operation: BossOperation,
  options?: RequestOptions,
): Promise<BossResult> {
  const response = await client.request(bossCommand(operation), undefined, undefined, options)
  return expectPayload(response, 'boss').result
}

function expectPayload<T extends ResponsePayload['type']>(
  response: ResponsePayload,
  expected: T,
): Extract<ResponsePayload, { type: T }> {
  if (response.type !== expected) {
    throw new Error(`Expected daemon response ${expected}, received ${response.type}`)
  }
  return response as Extract<ResponsePayload, { type: T }>
}

function expectResult<T extends BossResult['type']>(
  result: BossResult,
  expected: T,
): Extract<BossResult, { type: T }> {
  if (result.type !== expected) {
    throw new Error(`Expected boss result ${expected}, received ${result.type}`)
  }
  return result as Extract<BossResult, { type: T }>
}

/** The daemon's Boss document — employees, personas, deliverables, plans,
 * and the admission policy — or `null` when the daemon's boss experiment
 * is off. Mirrors the desktop's `load_remote_boss_state`: the settings
 * read runs first so an experiment-off daemon answers `null` instead of
 * the "Boss is disabled" operation error. Re-fetch on every
 * `taskStateChanged` broadcast — the daemon fires it for each boss
 * mutation, and `state.revision` tells a landing whether it moved. */
export async function loadBossState(
  client: BossClient,
  options?: RequestOptions,
): Promise<BossState | null> {
  const settings = await client.request({ type: 'getSettings' }, undefined, undefined, options)
  if (!expectPayload(settings, 'settings').settings.boss_experiment_enabled) return null
  const result = await bossRequest(client, { type: 'view' }, options)
  return expectResult(result, 'state').state
}

/** The boss surface a client can open: the boss chat's managed session id
 * plus the identity (name, avatar seed) its rows render. `sessionId` is
 * `null` until a client opens the chat — `openBossSession` creates it. */
export interface BossChat {
  identity: BossIdentity
  sessionId: string | null
}

export function bossChat(state: BossState): BossChat {
  return { identity: state.identity, sessionId: state.sessionId }
}

/** Open the boss chat — creates its managed session on first call,
 * reattaches it afterward. Human-only on the daemon; a paired client has
 * that authority. The returned session attaches through the ordinary
 * `attachSession` + `subscribe` path like any other task. */
export async function openBossSession(
  client: BossClient,
  options: { provider: ProviderKind; model?: string | null; mode: RuntimeMode },
  request?: RequestOptions,
): Promise<{ session: AgentSession; project: Project }> {
  const result = expectResult(
    await bossRequest(
      client,
      {
        type: 'open',
        provider: options.provider,
        model: options.model ?? null,
        mode: options.mode,
      },
      request,
    ),
    'session',
  )
  return { session: result.session, project: result.project }
}

/** A compact text digest of the roster — employee state without
 * transcripts, the same string `goddard-agent boss roster` prints. */
export async function bossRoster(client: BossClient, options?: RequestOptions): Promise<string> {
  const result = await bossRequest(client, { type: 'roster' }, options)
  return expectResult(result, 'roster').roster
}

/** A bounded transcript read — the whole session's condensed items, or one
 * turn when `turn` is set. `truncated` on the result marks entries the
 * total cap dropped from the front. */
export async function bossTranscript(
  client: BossClient,
  sessionId: string,
  turn?: number,
  options?: RequestOptions,
): Promise<AgentSessionTranscript> {
  const result = await bossRequest(
    client,
    { type: 'transcript', sessionId, turn: turn ?? null },
    options,
  )
  return expectResult(result, 'transcript').transcript
}

/** One directory of the boss files root — `''` lists the root itself.
 * Plan documents live under `plans/`, personas under `personas/`. */
export async function bossListFiles(
  client: BossClient,
  path = '',
  options?: RequestOptions,
): Promise<BossFile[]> {
  const result = await bossRequest(client, { type: 'listFiles', path }, options)
  return expectResult(result, 'files').files
}

/** A text file under the boss files root — the only read that resolves
 * `plans/` documents, which sit outside every project workspace. */
export async function bossReadFile(
  client: BossClient,
  path: string,
  options?: RequestOptions,
): Promise<string> {
  const result = await bossRequest(client, { type: 'readFile', path }, options)
  return expectResult(result, 'file').content
}

/** A plan's frozen-or-draft document. Accepts the `BossPlan` record or its
 * `planFile` directly; re-read when `BossState.revision` moves so agent
 * edits reach the reader. */
export function bossReadPlanDocument(
  client: BossClient,
  plan: BossPlan | string,
  options?: RequestOptions,
): Promise<string> {
  return bossReadFile(client, typeof plan === 'string' ? plan : plan.planFile, options)
}

async function bossSaved(
  client: BossClient,
  operation: BossOperation,
  options?: RequestOptions,
): Promise<void> {
  expectResult(await bossRequest(client, operation, options), 'saved')
}

/** Stamp a deliverable viewed — retires its unread marker. */
export function markDeliverableViewed(
  client: BossClient,
  id: string,
  options?: RequestOptions,
): Promise<void> {
  return bossSaved(client, { type: 'markDeliverableViewed', id }, options)
}

/** Pin (`true`) or unpin a deliverable; pinned rows lead the group and
 * never age out of it. */
export function pinDeliverable(
  client: BossClient,
  id: string,
  pinned = true,
  options?: RequestOptions,
): Promise<void> {
  return bossSaved(client, { type: 'pinDeliverable', id, pinned }, options)
}

/** Sweep a deliverable behind the dormant fold (`true`) or restore it to
 * the live list. */
export function sweepDeliverable(
  client: BossClient,
  id: string,
  dormant = true,
  options?: RequestOptions,
): Promise<void> {
  return bossSaved(client, { type: 'sweepDeliverable', id, dormant }, options)
}

/** Archive (`true`) or unarchive a deliverable — archived rows leave the
 * sidebar but keep their record, unlike `dismissDeliverable`. */
export function archiveDeliverable(
  client: BossClient,
  id: string,
  archived = true,
  options?: RequestOptions,
): Promise<void> {
  return bossSaved(client, { type: 'archiveDeliverable', id, archived }, options)
}

/** Remove a deliverable's row and its stored copy — destructive; prefer
 * `archiveDeliverable` when the record should survive. */
export function dismissDeliverable(
  client: BossClient,
  id: string,
  options?: RequestOptions,
): Promise<void> {
  return bossSaved(client, { type: 'dismissDeliverable', id }, options)
}

/** Stamp the Goals page viewed — goal finishes older than the stamp stop
 * counting toward unread, the same contract `viewedAt` gives deliverables. */
export function markBossGoalsViewed(
  client: BossClient,
  options?: RequestOptions,
): Promise<void> {
  return bossSaved(client, { type: 'markGoalsViewed' }, options)
}

/** Check off, drop, or reopen one item of a plan's work breakdown —
 * `inProgress` is derived and never set. `plan` accepts the `BossPlan::id`,
 * its planning-session id, or its `plans/<file>.md` path. Returns the
 * fresh document. */
export async function setPlanItemState(
  client: BossClient,
  plan: string,
  item: string,
  state: PlanItemState,
  options?: RequestOptions,
): Promise<BossState> {
  const result = await bossRequest(
    client,
    { type: 'setPlanItemState', plan, item, state },
    options,
  )
  return expectResult(result, 'state').state
}

/** Set a finalized plan's outcome — `completed`/`abandoned` close it,
 * `approved` reopens a closed plan. Every transition lands audited on the
 * record. Returns the fresh document. */
export async function setPlanOutcome(
  client: BossClient,
  plan: string,
  outcome: PlanOutcome,
  options?: RequestOptions,
): Promise<BossState> {
  const result = await bossRequest(
    client,
    { type: 'setPlanOutcome', plan, outcome },
    options,
  )
  return expectResult(result, 'state').state
}

/** An employee's admission lifecycle — `expired` stays authoritative for
 * records written before `state` existed, matching the daemon's
 * `BossEmployee::lifecycle` projection. A live lifecycle is the roster
 * half of runtime status; whether the session is mid-turn lives on the
 * task's `AgentSession.status`, keyed by `sessionId`. */
export function bossEmployeeLifecycle(employee: BossEmployee): EmployeeLifecycle {
  return employee.expired && employee.state !== 'expired' ? 'expired' : employee.state
}

/** An item's render state — stored state, or `inProgress` while at least
 * one employee linked to it is still live. Mirrors `PlanItem::progress`. */
export type PlanItemProgress = 'toDo' | 'inProgress' | 'done' | 'dropped'

export function planItemProgress(
  plan: BossPlan,
  item: PlanItem,
  employees: BossEmployee[],
): PlanItemProgress {
  if (item.state === 'done') return 'done'
  if (item.state === 'dropped') return 'dropped'
  const live = employees.some(
    (employee) =>
      employee.planId === plan.id &&
      employee.itemId === item.id &&
      !employee.expired,
  )
  return live ? 'inProgress' : 'toDo'
}

/** Where a deliverable's bytes can be browsed on the daemon host: the
 * record's `path` for a directory, its parent for a file. Read or list it
 * through the ordinary workspace operations (`browseDirectory`,
 * `listTree`, `readTextFile`, `readBinaryFile`). */
export interface DeliverableTarget {
  /** Absolute directory clients can browse. */
  root: string
  /** The file's name under `root`, or `null` when the deliverable is a directory. */
  relativePath: string | null
}

export function deliverableTarget(deliverable: BossDeliverable): DeliverableTarget {
  if (deliverable.directory) return { root: deliverable.path, relativePath: null }
  const separator = Math.max(
    deliverable.path.lastIndexOf('/'),
    deliverable.path.lastIndexOf('\\'),
  )
  if (separator < 0) return { root: deliverable.path, relativePath: null }
  return {
    root: separator === 0 ? deliverable.path.slice(0, 1) : deliverable.path.slice(0, separator),
    relativePath: deliverable.path.slice(separator + 1),
  }
}

/** The sidebar's unread rule for a deliverable row: archived rows are
 * never unread, a swept (dormant) row is unread only while pinned, and
 * otherwise the row is unread until `viewedAt` passes `updatedAt` — a
 * re-publish unreads it again. Mirrors `sidebar_deliverable_unread`. */
export function deliverableUnread(deliverable: BossDeliverable): boolean {
  return (
    deliverable.archivedAt == null &&
    (deliverable.dormantAt == null || deliverable.pinnedAt != null) &&
    (deliverable.viewedAt == null || deliverable.viewedAt < deliverable.updatedAt)
  )
}

/** A boss state change worth surfacing to the user, derived by diffing two
 * `BossState` snapshots — the daemon broadcasts `taskStateChanged` on
 * every boss mutation, so clients refetch `loadBossState` and diff rather
 * than carrying a separate event feed. Only transitions report: the first
 * sync (`previous` absent) and a boss-identity change report nothing, and
 * persistent conditions — an open blocker, an unread deliverable — are
 * read off the state itself. */
export type BossAttention =
  | { type: 'employeeBlocker'; employee: BossEmployee }
  | { type: 'deliverablePublished'; deliverable: BossDeliverable }
  | { type: 'deliverableUpdated'; deliverable: BossDeliverable }
  | { type: 'planFinalized'; plan: BossPlan }

export function bossAttention(
  previous: BossState | null | undefined,
  current: BossState,
): BossAttention[] {
  if (!previous || previous.identity.id !== current.identity.id) return []
  const attention: BossAttention[] = []
  for (const employee of current.employees) {
    if (employee.blocker == null) continue
    const before = previous.employees.find(
      (entry) => entry.sessionId === employee.sessionId,
    )
    if (before?.blocker == null) {
      attention.push({ type: 'employeeBlocker', employee })
    }
  }
  for (const deliverable of current.deliverables) {
    const before = previous.deliverables.find((entry) => entry.id === deliverable.id)
    if (!before) {
      attention.push({ type: 'deliverablePublished', deliverable })
    } else if (deliverable.updatedAt > before.updatedAt) {
      attention.push({ type: 'deliverableUpdated', deliverable })
    }
  }
  for (const plan of current.planning) {
    if (plan.finalizedAt == null) continue
    const before = previous.planning.find((entry) => entry.id === plan.id)
    if (before?.finalizedAt == null) {
      attention.push({ type: 'planFinalized', plan })
    }
  }
  return attention
}

/** The `WireDriverEvent.kind` a daemon emits on the boss session's event
 * stream when the boss asks the client to open a pinned, standalone
 * terminal. Arrives through the ordinary `subscribe(sessionId, runtimeId)`
 * stream — subscribe to the boss session the way any runtime is followed. */
export const BOSS_TERMINAL_INTENT_KIND = 'bossTerminalIntent'

export interface BossTerminalIntent {
  title: string
  cwd: string
  command?: string | null
}

/** Decode a `bossTerminalIntent` event — `null` for any other kind or a
 * malformed payload. */
export function bossTerminalIntent(event: WireDriverEvent): BossTerminalIntent | null {
  if (event.kind !== BOSS_TERMINAL_INTENT_KIND) return null
  const payload = event.payload
  if (typeof payload !== 'object' || payload === null) return null
  const record = payload as Record<string, unknown>
  if (typeof record.title !== 'string' || typeof record.cwd !== 'string') return null
  return {
    title: record.title,
    cwd: record.cwd,
    command: typeof record.command === 'string' ? record.command : null,
  }
}

/** Terminal intents emitted on one session stream — subscribe to the boss
 * chat's `sessionId`/`runtimeId` (from `bossChat` + `attachSession`). */
export function subscribeBossTerminalIntents(
  client: Pick<WakuClient, 'subscribe'>,
  sessionId: string,
  runtimeId: string,
  listener: (intent: BossTerminalIntent) => void,
): () => void {
  const wrap: EventListener = (event) => {
    const intent = bossTerminalIntent(event.event)
    if (intent) listener(intent)
  }
  return client.subscribe(sessionId, runtimeId, wrap)
}
