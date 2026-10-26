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
  test('groups recent local dates across week, month, year, and DST boundaries', () => {
    for (const now of [
      new Date(2026, 9, 26, 10),
      new Date(2026, 10, 2, 10),
      new Date(2027, 0, 4, 10),
      new Date(2026, 2, 10, 10),
    ]) {
      for (const [age, expected] of [
        [0, 'today'], [1, 'yesterday'], [2, 'week'], [6, 'week'],
        [7, 'month'], [29, 'month'],
      ] as const) {
        const updated = new Date(now.getFullYear(), now.getMonth(), now.getDate() - age, 18)
        expect(dateGroup(updated.getTime() / 1_000, now)).toBe(expected)
      }
      const older = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 30, 18)
      expect(dateGroup(older.getTime() / 1_000, now)).toBe(
        older.getFullYear() === now.getFullYear() ? 'year' : 'more',
      )
    }
    const monday = new Date(2026, 9, 26, 10)
    const saturday = new Date(2026, 9, 24, 18)
    expect(formatTimeAgo((monday.getTime() - saturday.getTime()) / 1_000)).toBe('1d')
    const groups = groupSessions([], [session({
      last_reply_at: saturday.getTime() / 1_000,
      messages: [{ id: 'message' } as never],
    })], monday)
    expect(groups[0]?.id).toBe('week')
    expect(groups[0]?.label).toBe('Last 7 days')
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
