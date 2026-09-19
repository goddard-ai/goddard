import type { BranchEntry } from '@waku/client'

// Fit dominates recency: a branch only outranks a better match when its tip
// commit is enough fresher to cover the match-quality gap. Mirrors
// `visible_branch_entries` in the native client's composer.
const MATCH_WEIGHT = 0.7
const RECENCY_WEIGHT = 0.3
const RECENCY_HALF_LIFE_SECS = 30 * 24 * 60 * 60

/** How well one query token fits a branch name, on a 0–1 scale. */
function tokenMatchScore(lowerName: string, token: string): number {
  const position = lowerName.indexOf(token)
  return position < 0 ? 0 : 1 - position / Math.max(1, lowerName.length)
}

/** The branch's recency on a 0–1 scale — exponential decay, fixed half-life. */
function recencyScore(lastCommitAt: number | null, nowUnixSecs: number): number {
  if (lastCommitAt == null) return 0
  const age = Math.max(0, nowUnixSecs - lastCommitAt)
  return 0.5 ** (age / RECENCY_HALF_LIFE_SECS)
}

/**
 * Branches matching the search, with an exact match first, then the selected
 * branch pinned, and every other row ranked by a blend of how closely the
 * name fits the query and how recently it was committed to. With an empty
 * query the blend reduces to recency order.
 */
export function visibleBranches(
  branches: BranchEntry[],
  selected: string | undefined,
  query: string,
  nowUnixSecs: number,
): BranchEntry[] {
  const normalized = query.trim().toLowerCase()
  const tokens = normalized.split(/\s+/).filter(Boolean)
  const rank = (branch: BranchEntry): number => {
    const lower = branch.name.toLowerCase()
    const fit = tokens.length
      ? tokens.reduce((sum, token) => sum + tokenMatchScore(lower, token), 0) / tokens.length
      : 0
    return MATCH_WEIGHT * fit + RECENCY_WEIGHT * recencyScore(branch.last_commit_at, nowUnixSecs)
  }
  return branches
    .filter((branch) => tokens.every((token) => branch.name.toLowerCase().includes(token)))
    .sort((left, right) => {
      const leftExact = left.name.toLowerCase() === normalized
      const rightExact = right.name.toLowerCase() === normalized
      if (leftExact !== rightExact) return leftExact ? -1 : 1
      if (left.name === selected) return -1
      if (right.name === selected) return 1
      const rankDelta = rank(right) - rank(left)
      return rankDelta !== 0 ? rankDelta : left.name.localeCompare(right.name)
    })
}
