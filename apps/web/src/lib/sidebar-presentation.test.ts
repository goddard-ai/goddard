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
  test('groups by floored elapsed days across midnight, year, and DST boundaries', () => {
    for (const now of [
      new Date(2026, 9, 26, 0, 30),
      new Date(2026, 10, 2, 10),
      new Date(2027, 0, 1, 0, 30),
      new Date(2026, 2, 9, 10),
    ]) {
      for (const [age, expected] of [
        [0, 'today'], [86_399, 'today'],
        [86_400, 'yesterday'], [2 * 86_400 - 1, 'yesterday'],
        [2 * 86_400, 'week'], [7 * 86_400 - 1, 'week'],
        [7 * 86_400, 'month'], [30 * 86_400 - 1, 'month'],
      ] as const) {
        expect(dateGroup(now.getTime() / 1_000 - age, now)).toBe(expected)
      }
      for (const age of [30, 365]) {
        const older = new Date(now.getTime() - age * 86_400_000)
        expect(dateGroup(older.getTime() / 1_000, now)).toBe(
          older.getFullYear() === now.getFullYear() ? 'year' : 'more',
        )
      }
      expect(dateGroup(now.getTime() / 1_000 + 86_400, now)).toBe('today')
    }
    const monday = new Date(2026, 9, 26, 10)
    const saturday = new Date(2026, 9, 24, 18)
    expect(formatTimeAgo((monday.getTime() - saturday.getTime()) / 1_000)).toBe('1d')
    const groups = groupSessions([], [session({
      last_reply_at: saturday.getTime() / 1_000,
      messages: [{ id: 'message' } as never],
    })], monday)
    expect(groups[0]?.id).toBe('yesterday')
    expect(groups[0]?.label).toBe('Yesterday')
  })

  test('updates unreplied task groups at elapsed-day boundaries', () => {
    const now = new Date(2026, 9, 26, 10).getTime() / 1_000
    const unreplied = session({
      created_at: now - 86_399,
      messages: [{ id: 'message' } as never],
    })
    expect(sessionTimeLabel(unreplied, now)).toBeNull()
    expect(nextSidebarUpdateDelay([unreplied], now)).toBe(1)
    expect(groupSessions([], [unreplied], new Date(now * 1_000))[0]?.id).toBe('today')
    expect(groupSessions([], [unreplied], new Date((now + 1) * 1_000))[0]?.id).toBe('yesterday')
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
