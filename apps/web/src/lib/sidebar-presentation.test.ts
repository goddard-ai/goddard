import { describe, expect, test } from 'bun:test'
import type { AgentSession, Project } from '@waku/client'
import {
  dateGroup,
  formatTimeAgo,
  groupSessions,
  nextSidebarUpdateDelay,
  sessionHasLiveTurn,
  sessionHasStarted,
  sessionTimeLabel,
  sidebarRows,
} from './sidebar-presentation'

describe('desktop sidebar presentation', () => {
  test('uses the selected local day start for Today and Yesterday', () => {
    const now = new Date(2026, 9, 27, 12)
    for (let hour = 0; hour <= 6; hour++) {
      const today = new Date(2026, 9, 27, hour).getTime() / 1_000
      const yesterday = new Date(2026, 9, 26, hour).getTime() / 1_000
      expect(dateGroup(today, now, hour)).toBe('today')
      expect(dateGroup(today - 1, now, hour)).toBe('yesterday')
      expect(dateGroup(yesterday, now, hour)).toBe('yesterday')
      expect(dateGroup(yesterday - 1, now, hour)).toBe('week')
      const before = new Date((today - 1) * 1_000)
      expect(dateGroup(yesterday, before, hour)).toBe('today')
      expect(nextSidebarUpdateDelay([], today - 1, hour)).toBe(1)
    }
    const lateNight = new Date(2026, 9, 27, 2)
    const evening = new Date(2026, 9, 26, 23).getTime() / 1_000
    expect(dateGroup(evening, lateNight)).toBe('today')
    expect(dateGroup(evening, lateNight, 0)).toBe('yesterday')
    expect(groupSessions([], [session({ last_reply_at: evening, messages: [{ id: 'message' } as never] })], lateNight)[0]?.id).toBe('today')
  })

  test('keeps older groups on floored elapsed ages across year and DST boundaries', () => {
    for (const now of [new Date(2026, 10, 2, 10), new Date(2027, 0, 1, 5), new Date(2026, 2, 9, 10)]) {
      for (const [age, expected] of [
        [3 * 86_400, 'week'], [7 * 86_400 - 1, 'week'],
        [7 * 86_400, 'month'], [30 * 86_400 - 1, 'month'],
      ] as const) {
        expect(dateGroup(now.getTime() / 1_000 - age, now)).toBe(expected)
      }
    }
  })

  test('rolls groups over at the local cutoff through DST and year changes', () => {
    for (const [month, day] of [[2, 7], [10, 1], [11, 31]] as const) {
      const now = new Date(2026, month, day, 4)
      const next = new Date(2026, month, day + 1, 4)
      expect(nextSidebarUpdateDelay([], now.getTime() / 1_000)).toBe((next.getTime() - now.getTime()) / 1_000)
      expect(dateGroup(now.getTime() / 1_000, next)).toBe('yesterday')
    }
    const beforeGap = new Date(2026, 2, 8, 1, 59, 59)
    const gapEnd = new Date(2026, 2, 8, 2)
    expect(nextSidebarUpdateDelay([], beforeGap.getTime() / 1_000, 2)).toBe((gapEnd.getTime() - beforeGap.getTime()) / 1_000)
    const firstOne = new Date(2026, 10, 1, 1)
    const repeatedOne = new Date(firstOne.getTime() + 3_600_000)
    if (repeatedOne.getHours() === 1) {
      expect(nextSidebarUpdateDelay([], firstOne.getTime() / 1_000 - 1, 1)).toBe(1)
      const tomorrow = new Date(2026, 10, 2, 1)
      expect(nextSidebarUpdateDelay([], repeatedOne.getTime() / 1_000, 1)).toBe((tomorrow.getTime() - repeatedOne.getTime()) / 1_000)
      expect(dateGroup(firstOne.getTime() / 1_000, repeatedOne, 1)).toBe('today')
    }
  })

  test('keeps Add Project in an empty history through the first group header', () => {
    expect(sidebarRows([], new Set())).toEqual([
      { kind: 'search', key: 'search' },
      {
        kind: 'group',
        key: 'group:today',
        group: { id: 'today', label: 'Today', sessions: [] },
        collapsed: false,
        first: true,
      },
    ])
  })

  test('matches desktop settled and live time labels', () => {
    expect(formatTimeAgo(0)).toBe('just now')
    expect(formatTimeAgo(604_800)).toBe('7d')

    const live = session({
      status: 'working',
      last_reply_at: 100,
      turns: [{
        id: 'turn',
        turn_count: 1,
        status: 'running',
        provider_turn_started: true,
        started_at: 100,
        completed_at: null,
        checkpoint: null,
      }],
    })
    expect(sessionHasLiveTurn(live)).toBe(true)
    // A live turn keeps the reply age on the same minute-boundary clock.
    expect(sessionTimeLabel(live, 165)).toBe('1m')
    expect(nextSidebarUpdateDelay([live], 165)).toBe(55)
  })

  test('does not invent a reply time and keeps cursor-only resumed tasks', () => {
    const resumed = session({ provider_cursor: { provider: 'codex', value: {} } as never })
    expect(sessionHasStarted(resumed)).toBe(true)
    expect(sessionTimeLabel(resumed, 1_000)).toBeNull()
  })

  test('presents the projectless sentinel with the localized desktop name', () => {
    const project: Project = {
      id: 'project',
      name: 'No project',
      path: '/home/me/.waku/projects/session',
      created_at: 1,
      temporary: false,
      starred: false,
      submissions_enabled: false,
    }
    const groups = groupSessions(
      [project],
      [session({ messages: [{ id: 'message' } as never] })],
      new Date(2026, 7, 15, 12),
      'Unknown project',
      'プロジェクトなし',
    )
    expect(groups[0]?.sessions[0]?.projectName).toBe('プロジェクトなし')
  })
})

function session(patch: Partial<AgentSession>): AgentSession {
  return {
    id: 'session',
    title: 'New Task',
    project_id: 'project',
    provider: 'codex',
    model: null,
    runtime_mode: 'ask',
    status: 'idle',
    created_at: 10,
    updated_at: 10,
    provider_cursor: null,
    messages: [],
    transcript_blocks: [],
    turns: [],
    ...patch,
  }
}
