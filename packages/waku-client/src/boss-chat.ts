import type {
  ActivityItem,
  AgentSession,
  CustomCommandIcon,
  Message,
  ReportTrigger,
} from './generated'

/**
 * Boss-chat transcript presentation: the pieces of the supervisor session's
 * wire data that render as bespoke rows rather than ordinary messages or
 * tool groups — summon cards, report-trigger markers, and the rotation
 * chain's predecessor link. Pure data; the client package stays free of UI.
 */

/** The `tool_name` the daemon stamps on the transcript activity it records
 * into a supervisor's session when a `BossOperation::Summon` succeeds —
 * mirrors `BOSS_SUMMON_TOOL_NAME` in waku-protocol's model. */
export const BOSS_SUMMON_TOOL_NAME = 'boss_summon'

/** The summon-time identity a `boss_summon` marker's `arguments` carries:
 * the roster stays authoritative while the employee is on it; this payload
 * is the fallback once the record retires or the task archives. */
export interface BossSummonCard {
  sessionId: string
  name: string
  avatarSeed: string
  jobTitle: string
  icon: CustomCommandIcon | null
}

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/** Reads a `boss_summon` marker's `arguments`: the JSON card newer daemons
 * write, or a bare session id from older markers — whose only certain
 * identity is the id itself, which also seeded the avatar at summon. */
export function bossSummonCard(activity: ActivityItem): BossSummonCard | null {
  if (activity.tool_name !== BOSS_SUMMON_TOOL_NAME) return null
  const args = activity.arguments?.trim()
  if (!args) return null
  if (UUID_PATTERN.test(args)) {
    return { sessionId: args, name: '', avatarSeed: args, jobTitle: '', icon: null }
  }
  try {
    const parsed = JSON.parse(args) as Partial<BossSummonCard>
    if (typeof parsed.sessionId !== 'string' || !parsed.sessionId) return null
    return {
      sessionId: parsed.sessionId,
      name: typeof parsed.name === 'string' ? parsed.name : '',
      avatarSeed:
        typeof parsed.avatarSeed === 'string' ? parsed.avatarSeed : parsed.sessionId,
      jobTitle: typeof parsed.jobTitle === 'string' ? parsed.jobTitle : '',
      icon: parsed.icon ?? null,
    }
  } catch {
    return null
  }
}

/** The employee session a `boss_summon` marker names — `null` for any other
 * activity. */
export function bossSummonSessionId(activity: ActivityItem): string | null {
  return bossSummonCard(activity)?.sessionId ?? null
}

/** One report-trigger marker row: the run of same-boundary reports that
 * collapse into it, anchored at the first member's position. */
export interface BossTriggerGroup {
  /** Message id of the run's first member — the row's position and key. */
  anchorId: string
  boundary: 'opening' | 'steer'
  /** Event-deduped triggers in document order. */
  triggers: ReportTrigger[]
}

/** Whether boss output — a message or a transcript block that renders —
 * separates the messages at `lo` and `hi`. Hidden prompts are invisible,
 * so an accepted run closes over them; another marker still separates.
 * Mirrors desktop's `steer_run_separated`. */
function steerRunSeparated(session: AgentSession, lo: number, hi: number): boolean {
  for (let index = lo + 1; index < hi; index += 1) {
    const message = session.messages[index]!
    if (!message.hidden || message.report_trigger?.boundary === 'opening') {
      return true
    }
  }
  return session.transcript_blocks.some(
    (block) => block.after_message > lo && block.after_message <= hi,
  )
}

/**
 * The marker rows the transcript draws for `report_trigger` messages, in
 * document order. An opening report joins every other opening trigger of
 * the same turn (a burst lands when a wave settles together); an accepted
 * steer joins the contiguous run of hidden steer prompts around it. Both
 * collapse to one row anchored at the run's first member — the row is the
 * quiet "what woke this turn" record, not per-message chrome.
 *
 * Mirrors `boss_trigger_group` in the desktop transcript. Every hidden
 * report-trigger message belongs to exactly one group; callers render a
 * marker at each `anchorId` and nothing for the rest.
 */
export function bossTriggerGroups(session: AgentSession): BossTriggerGroup[] {
  const isSteerMember = (message: Message) =>
    message.hidden === true && message.report_trigger?.boundary === 'steer'

  const groups: BossTriggerGroup[] = []
  const claimed = new Set<number>()
  const dedupe = (members: number[]): ReportTrigger[] => {
    const seen = new Set<string>()
    const triggers: ReportTrigger[] = []
    for (const index of members) {
      const trigger = session.messages[index]!.report_trigger
      if (trigger && !seen.has(trigger.eventId)) {
        seen.add(trigger.eventId)
        triggers.push(trigger)
      }
    }
    return triggers
  }

  session.messages.forEach((message, index) => {
    const trigger = message.report_trigger
    if (!trigger || claimed.has(index)) return

    if (trigger.boundary === 'opening') {
      const members: number[] = []
      session.messages.forEach((candidate, candidateIndex) => {
        if (
          candidate.turn_id === message.turn_id
          && candidate.report_trigger?.boundary === 'opening'
        ) {
          members.push(candidateIndex)
        }
      })
      if (members[0] !== index) return
      for (const member of members) claimed.add(member)
      groups.push({ anchorId: message.id, boundary: 'opening', triggers: dedupe(members) })
      return
    }

    // Steer: walk back to the run's start; only the first member anchors.
    let first = index
    while (first > 0) {
      const previous = session.messages[first - 1]!
      if (!isSteerMember(previous) || steerRunSeparated(session, first - 1, first)) break
      first -= 1
    }
    if (first !== index) {
      // A later member anchors nothing; the anchor already claimed it.
      return
    }
    const members = [index]
    let next = index + 1
    while (next < session.messages.length) {
      const candidate = session.messages[next]!
      if (!isSteerMember(candidate) || steerRunSeparated(session, next - 1, next)) break
      members.push(next)
      next += 1
    }
    for (const member of members) claimed.add(member)
    groups.push({ anchorId: message.id, boundary: 'steer', triggers: dedupe(members) })
  })

  return groups
}

/** The event-time label for one report: `{name} {outcome phrase}`, plus
 * " during this turn" when the provider accepted it into a turn already
 * open. `name` falls back to a generic employee or plan — never a UUID.
 * Mirrors `boss_trigger_entry_label` (English). */
export function bossTriggerEntryLabel(trigger: ReportTrigger, duringTurn: boolean): string {
  const name = trigger.employeeName || (trigger.kind === 'planFinalized' ? 'Plan' : 'Employee')
  const label = (() => {
    switch (trigger.kind) {
      case 'finished':
        return `${name} finished`
      case 'failed':
        return `${name} failed`
      case 'blocker':
        return `${name} reported a blocker`
      case 'finishedWithBlocker':
        return `${name} finished with a blocker`
      case 'interrupted':
        return `${name} was interrupted`
      case 'planFinalized':
        return `${name} finalized its design`
    }
  })()
  return duringTurn ? `${label} during this turn` : label
}

/** The collapsed burst header: one uniform outcome from distinct employees
 * names them, anything else counts updates and calls out failures and
 * blockers. Mirrors `boss_trigger_burst_label` (English). */
export function bossTriggerBurstLabel(triggers: ReportTrigger[]): string {
  const count = triggers.length
  const distinct = new Set(triggers.map((trigger) => trigger.employee)).size === count
  const all = (kind: ReportTrigger['kind']) => triggers.every((trigger) => trigger.kind === kind)
  if (distinct && all('finished')) return `${count} employees finished`
  if (distinct && all('failed')) return `${count} employees failed`
  if (distinct && all('planFinalized')) return `${count} plans finalized`

  let label = `${count} employee updates`
  const failed = triggers.filter((trigger) => trigger.kind === 'failed').length
  if (failed) label += ` · ${failed} failed`
  const blockers = triggers.filter(
    (trigger) =>
      trigger.kind === 'blocker'
      || trigger.kind === 'finishedWithBlocker'
      || trigger.kind === 'interrupted',
  ).length
  if (blockers) label += ` · ${blockers} ${blockers === 1 ? 'blocker' : 'blockers'}`
  return label
}

export type BossTriggerTone = 'tertiary' | 'warning' | 'danger'

/** The glyph family one report kind leads with — the icon slot's tone only;
 * the words carry the meaning. Mirrors `boss_trigger_status`. */
export function bossTriggerTone(kind: ReportTrigger['kind']): BossTriggerTone {
  switch (kind) {
    case 'finished':
    case 'planFinalized':
      return 'tertiary'
    case 'failed':
      return 'danger'
    case 'blocker':
    case 'finishedWithBlocker':
    case 'interrupted':
      return 'warning'
  }
}

/** The burst header's tone: danger when any report failed, warning when any
 * flagged a blocker or interruption, tertiary otherwise. Mirrors
 * `boss_trigger_group_status`. */
export function bossTriggerGroupTone(triggers: ReportTrigger[]): BossTriggerTone {
  if (triggers.some((trigger) => trigger.kind === 'failed')) return 'danger'
  if (triggers.some(
    (trigger) =>
      trigger.kind === 'blocker'
      || trigger.kind === 'finishedWithBlocker'
      || trigger.kind === 'interrupted',
  )) {
    return 'warning'
  }
  return 'tertiary'
}

/** A bare name in prose that resolves to a task — the way a boss
 * transcript refers to an employee. A word-bounded match in an eligible
 * run renders as a tappable chip. Mirrors desktop `SessionMention`. */
export interface BossSessionMention {
  name: string
  session: string
}

export interface BossMentionSpan {
  start: number
  end: number
  session: string
}

function isWordChar(ch: string | undefined): boolean {
  return ch != null && /[\p{L}\p{N}_]/u.test(ch)
}

/**
 * Word-bounded mention hits in one run's text, sorted and non-overlapping —
 * the pieces a text leaf splits into. `prevWord` is whether the character
 * before the run is a word character, since a run boundary is not a word
 * boundary. Matching is case-sensitive and exact, like the desktop's
 * `find_session_mentions`.
 */
export function findBossSessionMentions(
  text: string,
  mentions: readonly BossSessionMention[],
  prevWord: boolean,
): BossMentionSpan[] {
  const hits: BossMentionSpan[] = []
  for (const mention of mentions) {
    const name = mention.name
    if (!name) continue
    let from = 0
    for (;;) {
      const offset = text.indexOf(name, from)
      if (offset < 0) break
      const start = offset
      const end = start + name.length
      const beforeOk = start === 0 ? !prevWord : !isWordChar(text[start - 1])
      const afterOk = !isWordChar(text[end])
      if (beforeOk && afterOk) hits.push({ start, end, session: mention.session })
      from = start + 1
    }
  }
  hits.sort((a, b) => (a.start - b.start) || (b.end - a.end))
  const out: BossMentionSpan[] = []
  for (const hit of hits) {
    const last = out[out.length - 1]
    if (last && hit.start < last.end) continue
    out.push(hit)
  }
  return out
}

const ROTATION_LINK_PREFIX = '<boss-rotation-link:'
const ROTATION_LINK_SUFFIX = '>'

/**
 * The session the daemon rotated into this boss chat, when the marker row
 * it wrote into this session names one. Only the destination's marker is a
 * predecessor link; the source's outgoing marker must be ignored — the
 * hidden form exists only on the destination, and the legacy visible form
 * is checked against this session's id.
 *
 * Mirrors `boss_chat_predecessor`.
 */
export function bossChatPredecessor(session: AgentSession): string | null {
  for (const message of session.messages) {
    if (message.role !== 'system') continue
    let old: string | null = null
    if (message.hidden) {
      if (
        message.content.startsWith(ROTATION_LINK_PREFIX)
        && message.content.endsWith(ROTATION_LINK_SUFFIX)
      ) {
        old = message.content.slice(ROTATION_LINK_PREFIX.length, -ROTATION_LINK_SUFFIX.length)
      }
    } else {
      // Transcripts written before rotation links became hidden metadata.
      const ids = message.content.split('. Old session: ')[1]?.split('. New session: ')
      if (ids && ids.length >= 2 && ids[1]!.split('.')[0] === session.id) {
        old = ids[0]!
      }
    }
    if (old && UUID_PATTERN.test(old) && old !== session.id) return old
  }
  return null
}
