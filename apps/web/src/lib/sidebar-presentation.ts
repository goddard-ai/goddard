import type { AgentSession, Project } from '@waku/client'
import { projectDisplayName } from './project-presentation'
import { DEFAULT_SIDEBAR_DAY_START_HOUR } from './sidebar-preferences'

export type DateGroup = 'today' | 'yesterday' | 'week' | 'month' | 'year' | 'more'

export interface SessionItem {
  session: AgentSession
  projectName: string
  /** Sender's display name for friend-delivered sessions — the row shows a person icon and this name. */
  friendName: string | null
  /** The project is a friend-delivery group. */
  projectIsFriend: boolean
  timestamp: number
}

export interface SessionGroup {
  id: DateGroup
  label: string
  sessions: SessionItem[]
}

export type SidebarListRow =
  | { kind: 'search'; key: 'search' }
  | { kind: 'group'; key: string; group: SessionGroup; collapsed: boolean; first: boolean }
  | { kind: 'session'; key: string; item: SessionItem }
  | { kind: 'spacer'; key: string }

const GROUP_LABELS: Record<DateGroup, string> = {
  today: 'Today',
  yesterday: 'Yesterday',
  week: 'Last 7 days',
  month: 'Last 30 days',
  year: 'This Year',
  more: 'More',
}

const GROUP_ORDER: DateGroup[] = ['today', 'yesterday', 'week', 'month', 'year', 'more']

export function sidebarRows(
  groups: SessionGroup[],
  collapsed: ReadonlySet<DateGroup>,
): SidebarListRow[] {
  const rows: SidebarListRow[] = [{ kind: 'search', key: 'search' }]
  const visibleGroups = groups.length
    ? groups
    // Desktop keeps the first header so Add Project never disappears merely
    // because there is no task history yet.
    : [{ id: 'today' as const, label: GROUP_LABELS.today, sessions: [] }]
  visibleGroups.forEach((group, index) => {
    const isCollapsed = collapsed.has(group.id)
    rows.push({
      kind: 'group',
      key: `group:${group.id}`,
      group,
      collapsed: isCollapsed,
      first: index === 0,
    })
    if (!isCollapsed) {
      rows.push(...group.sessions.map((item) => ({
        kind: 'session' as const,
        key: `session:${item.session.id}`,
        item,
      })))
    }
    if (groups.length) rows.push({ kind: 'spacer', key: `spacer:${group.id}` })
  })
  return rows
}

export function groupSessions(
  projects: Project[],
  sessions: AgentSession[],
  now = new Date(),
  unknownProject = 'Unknown project',
  projectlessName = 'No project',
  dayStartHour = DEFAULT_SIDEBAR_DAY_START_HOUR,
): SessionGroup[] {
  const projectNames = new Map(projects.map((project) => [
    project.id,
    projectDisplayName(project, projectlessName),
  ]))
  const friendProjects = new Set(
    projects.filter((project) => project.kind === 'friends' || project.friend_peer_id != null).map((project) => project.id),
  )
  const grouped = new Map<DateGroup, SessionItem[]>()
  const started = sessions
    .filter(sessionHasStarted)
    .sort((left, right) => sessionTimestamp(right) - sessionTimestamp(left))
  for (const session of started) {
    const id = dateGroup(sessionTimestamp(session), now, dayStartHour)
    const items = grouped.get(id) ?? []
    items.push({
      session,
      projectName: projectNames.get(session.project_id) ?? unknownProject,
      friendName: session.friend_peer_name ?? null,
      projectIsFriend: friendProjects.has(session.project_id),
      timestamp: sessionTimestamp(session),
    })
    grouped.set(id, items)
  }
  return GROUP_ORDER
    .filter((id) => grouped.has(id))
    .map((id) => ({ id, label: GROUP_LABELS[id], sessions: grouped.get(id)! }))
}

export function sessionHasStarted(session: AgentSession): boolean {
  return Boolean(
    session.turns.length
      || session.messages.length
      || session.provider_cursor
      || session.last_reply_at,
  )
}

export function sessionHasLiveTurn(session: AgentSession): boolean {
  const turn = session.turns.at(-1)
  return (
    (session.status === 'connecting' || session.status === 'working' || session.status === 'waiting' || session.status === 'background')
      && turn?.status === 'running'
  )
}

export function sessionTimeLabel(
  session: AgentSession,
  nowSeconds = Math.floor(Date.now() / 1_000),
  t?: Translator,
): string | null {
  // The reply age shows through a live turn too; a session that has never
  // replied shows nothing.
  if (session.last_reply_at == null) return null
  const elapsed = Math.max(0, nowSeconds - session.last_reply_at)
  return t ? formatTimeAgoLocalized(elapsed, t) : formatTimeAgo(elapsed)
}

export function nextSidebarUpdateDelay(
  sessions: AgentSession[],
  nowSeconds = Math.floor(Date.now() / 1_000),
  dayStartHour = DEFAULT_SIDEBAR_DAY_START_HOUR,
): number {
  const now = new Date(nowSeconds * 1_000)
  const today = sidebarDayStart(now, dayStartHour)
  const nextStart = new Date(today.getFullYear(), today.getMonth(), today.getDate() + 1, dayStartHour)
  let next = Math.max(1, Math.ceil(nextStart.getTime() / 1_000 - nowSeconds))
  for (const session of sessions) {
    if (session.last_reply_at == null && !sessionHasStarted(session)) continue
    const elapsed = Math.max(0, nowSeconds - sessionTimestamp(session))
    if (session.last_reply_at == null && elapsed >= 30 * 86_400) continue
    const step = session.last_reply_at == null
      ? 86_400
      : elapsed < 3_600 ? 60 : elapsed < 86_400 ? 3_600 : 86_400
    const remaining = Math.max(1, step - elapsed % step)
    next = Math.min(next, remaining)
  }
  return next
}

export function dateGroup(timestamp: number, now = new Date(), dayStartHour = DEFAULT_SIDEBAR_DAY_START_HOUR): DateGroup {
  const today = sidebarDayStart(now, dayStartHour)
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1, dayStartHour)
  if (timestamp * 1_000 >= today.getTime()) return 'today'
  if (timestamp * 1_000 >= yesterday.getTime()) return 'yesterday'
  const ageDays = Math.floor(Math.max(0, Math.floor(now.getTime() / 1_000) - timestamp) / 86_400)
  if (ageDays < 7) return 'week'
  if (ageDays < 30) return 'month'
  if (new Date(timestamp * 1_000).getFullYear() === now.getFullYear()) return 'year'
  return 'more'
}

export function formatTimeAgo(seconds: number): string {
  if (seconds < 60) return 'just now'
  if (seconds < 3_600) return `${Math.floor(seconds / 60)}m`
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)}h`
  return `${Math.floor(seconds / 86_400)}d`
}

function formatTimeAgoLocalized(seconds: number, t: Translator): string {
  if (seconds < 60) return t('sidebar.just_now')
  if (seconds < 3_600) return t('sidebar.minutes_ago', { count: Math.floor(seconds / 60) })
  if (seconds < 86_400) return t('sidebar.hours_ago', { count: Math.floor(seconds / 3_600) })
  return t('sidebar.days_ago', { count: Math.floor(seconds / 86_400) })
}

type Translator = (key: string, params?: Record<string, string | number>) => string

function sessionTimestamp(session: AgentSession): number {
  return session.last_reply_at ?? session.created_at
}

function sidebarDayStart(now: Date, hour: number): Date {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour)
  if (today.getTime() <= now.getTime()) return today
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1, hour)
}
