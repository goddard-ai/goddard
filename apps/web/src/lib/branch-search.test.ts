import { describe, expect, test } from 'bun:test'
import type { BranchEntry } from '@waku/client'
import { visibleBranches } from './branch-search'

const NOW = 1_800_000_000
const DAY = 86_400

function branch(name: string, lastCommitAt: number | null, checkedOutElsewhere = false): BranchEntry {
  return { name, checked_out_elsewhere: checkedOutElsewhere, last_commit_at: lastCommitAt }
}

function names(entries: BranchEntry[]): string[] {
  return entries.map((entry) => entry.name)
}

describe('branch picker search', () => {
  test('pins the selection and filters by name', () => {
    const branches = [
      branch('topic/zebra', NOW - 90 * DAY),
      branch('main', NOW - 30 * DAY),
      branch('topic/apple', NOW - DAY, true),
    ]
    expect(names(visibleBranches(branches, 'main', '', NOW))).toEqual([
      'main',
      'topic/apple',
      'topic/zebra',
    ])
    expect(names(visibleBranches(branches, 'main', 'TOPIC APPLE', NOW))).toEqual(['topic/apple'])
  })

  test('prefers exact matches', () => {
    const branches = [
      branch('mainline', NOW - DAY),
      branch('topic/main', NOW - DAY),
      branch('main', NOW - DAY),
    ]
    expect(names(visibleBranches(branches, 'topic/main', 'MAIN', NOW))).toEqual([
      'main',
      'topic/main',
      'mainline',
    ])
    expect(names(visibleBranches(branches, 'topic/main', 'mai', NOW))).toEqual([
      'topic/main',
      'main',
      'mainline',
    ])
  })

  test('blends match fit with recency', () => {
    const branches = [
      branch('topic/fix', NOW - DAY),
      branch('fix-old', NOW - 2 * 365 * DAY),
      branch('x-fix', NOW - DAY),
    ]
    // A fresh branch outranks a stale one with a marginally better fit, while
    // a large fit gap still beats recency.
    expect(names(visibleBranches(branches, undefined, 'fix', NOW))).toEqual([
      'x-fix',
      'fix-old',
      'topic/fix',
    ])
    // Without a query the ranking reduces to last-committed order.
    expect(names(visibleBranches(branches, undefined, '', NOW))).toEqual([
      'topic/fix',
      'x-fix',
      'fix-old',
    ])
  })
})
