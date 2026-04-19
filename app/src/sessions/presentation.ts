import type { DaemonSession } from "@goddard-ai/sdk"

function basename(path: string) {
  const normalized = path.replace(/[\\/]+$/, "")
  const segments = normalized.split(/[\\/]/)
  return segments.at(-1) || path
}

function normalizeSearchText(value: string) {
  return value.trim().toLowerCase()
}

export function getSessionDisplayTitle(session: DaemonSession) {
  return (
    session.title.trim() ||
    session.initiative?.trim() ||
    session.repository ||
    basename(session.cwd)
  )
}

export function getSessionRepositoryLabel(session: DaemonSession) {
  return session.repository?.trim() || basename(session.cwd)
}

function scoreSessionTitleMatch(session: DaemonSession, searchQuery: string) {
  const title = normalizeSearchText(getSessionDisplayTitle(session))

  if (title.length === 0) {
    return null
  }

  if (title.includes(searchQuery)) {
    return 10_000 - title.indexOf(searchQuery) * 100 - (title.length - searchQuery.length)
  }

  let searchStart = 0
  let firstMatchIndex = -1
  let lastMatchIndex = -1
  let streak = 0
  let score = 0

  for (const char of searchQuery) {
    const matchIndex = title.indexOf(char, searchStart)

    if (matchIndex < 0) {
      return null
    }

    const previousChar = title[matchIndex - 1] ?? " "
    const startsWord =
      matchIndex === 0 ||
      previousChar === " " ||
      previousChar === "-" ||
      previousChar === "_" ||
      previousChar === "/"

    if (firstMatchIndex < 0) {
      firstMatchIndex = matchIndex
    }

    score += startsWord ? 12 : 4

    if (lastMatchIndex === matchIndex - 1) {
      streak += 1
      score += 10 + Math.min(streak, 4)
    } else {
      streak = 0
    }

    searchStart = matchIndex + 1
    lastMatchIndex = matchIndex
  }

  return score - firstMatchIndex * 2 - (title.length - searchQuery.length)
}

export function filterSessionsByTitle(sessions: readonly DaemonSession[], searchQuery: string) {
  const normalizedQuery = normalizeSearchText(searchQuery)

  if (normalizedQuery.length === 0) {
    return sessions
  }

  return sessions
    .map((session, index) => ({
      session,
      index,
      score: scoreSessionTitleMatch(session, normalizedQuery),
    }))
    .filter(
      (entry): entry is { session: DaemonSession; index: number; score: number } =>
        entry.score !== null,
    )
    .sort((left, right) => {
      if (left.score !== right.score) {
        return right.score - left.score
      }

      if (left.session.updatedAt !== right.session.updatedAt) {
        return right.session.updatedAt - left.session.updatedAt
      }

      return left.index - right.index
    })
    .map((entry) => entry.session)
}
