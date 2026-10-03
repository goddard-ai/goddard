import type { Project } from './generated/Project'
import type { SessionMessageSearchScope } from './generated/SessionMessageSearchScope'
import type { SessionStatus } from './generated/SessionStatus'

/**
 * A session-message search after its `field:value` filters are lifted out
 * of the raw query — the TypeScript twin of
 * `waku_protocol::persistence::parse_session_message_search`, kept to the
 * same grammar so the web palette and `goddard-agent search` read a query
 * identically.
 *
 * `project:<name-or-id>`, `status:<status>`, `archived:<true|any|false>`,
 * and `limit:<n>` are filters. Any other token — including a `field:` token
 * whose name or value is not recognized — stays in `text`, so a stray
 * qualifier degrades to a literal search instead of an error. Values may be
 * double-quoted to hold whitespace (`project:"my app"`); repeated
 * `project:`/`status:` tokens union, `archived:`/`limit:` take the last
 * value, and different filters intersect.
 */
export interface SessionMessageSearchQuery {
  /** The remaining free text: one case-insensitive substring needle. */
  text: string
  /** `project:` values as typed — callers resolve them to project ids. */
  projects: string[]
  /** `status:` values; `busy` expands to the busy set. */
  statuses: SessionStatus[]
  /** `archived:` override; `undefined` keeps the caller's scope. */
  scope?: SessionMessageSearchScope
  /** `limit:` override; `undefined` keeps the caller's cap. */
  limit?: number
}

const BUSY_STATUSES: readonly SessionStatus[] = [
  'connecting',
  'working',
  'waiting',
  'background',
]

const STATUS_VALUES: Readonly<Record<string, readonly SessionStatus[]>> = {
  idle: ['idle'],
  connecting: ['connecting'],
  working: ['working'],
  waiting: ['waiting'],
  background: ['background'],
  failed: ['failed'],
  busy: BUSY_STATUSES,
}

/** Split a query into whitespace-separated tokens without breaking inside
 * double quotes — `project:"my app"` stays one token, as does the phrase
 * `"my app"`. */
function sessionSearchTokens(query: string): string[] {
  const tokens: string[] = []
  let index = 0
  while (index < query.length) {
    while (index < query.length && /\s/.test(query[index]!)) index += 1
    if (index >= query.length) break
    const start = index
    let quoted = false
    while (index < query.length) {
      const ch = query[index]!
      if (ch === '"') quoted = !quoted
      else if (/\s/.test(ch) && !quoted) break
      index += 1
    }
    tokens.push(query.slice(start, index))
  }
  return tokens
}

/** Strip the quotes of a `"…"`-quoted value; a bare value is returned as is. */
function unquote(value: string): string {
  if (!value.startsWith('"')) return value
  const inner = value.slice(1)
  return inner.endsWith('"') ? inner.slice(0, -1) : inner
}

/** Apply one `name:value` token to `parsed`, returning whether the token was
 * a recognized filter. A known field with an unusable value returns `false`
 * so the token falls back to literal search text. */
function applySessionSearchFilter(
  name: string,
  value: string,
  parsed: SessionMessageSearchQuery,
): boolean {
  const unquoted = unquote(value)
  switch (name.toLowerCase()) {
    case 'project':
      if (!unquoted) return false
      parsed.projects.push(unquoted)
      return true
    case 'status': {
      const statuses = STATUS_VALUES[unquoted.toLowerCase()]
      if (!statuses) return false
      for (const status of statuses) {
        if (!parsed.statuses.includes(status)) parsed.statuses.push(status)
      }
      return true
    }
    case 'archived':
      switch (unquoted.toLowerCase()) {
        case 'true':
          parsed.scope = 'archived'
          return true
        case 'any':
        case 'all':
          parsed.scope = 'any'
          return true
        case 'false':
          parsed.scope = 'active'
          return true
        default:
          return false
      }
    case 'limit': {
      if (!/^\d+$/.test(unquoted)) return false
      parsed.limit = Number.parseInt(unquoted, 10)
      return true
    }
    default:
      return false
  }
}

export function parseSessionMessageSearch(query: string): SessionMessageSearchQuery {
  const parsed: SessionMessageSearchQuery = {
    text: '',
    projects: [],
    statuses: [],
  }
  const text: string[] = []
  for (const raw of sessionSearchTokens(query)) {
    const colon = raw.indexOf(':')
    const name = colon > 0 ? raw.slice(0, colon) : null
    const consumed = name !== null
      && /^[a-zA-Z]+$/.test(name)
      && applySessionSearchFilter(name, raw.slice(colon + 1), parsed)
    if (consumed) continue
    text.push(unquote(raw))
  }
  parsed.text = text.join(' ')
  return parsed
}

/** Whether anything — text or filter — constrains the search. */
export function sessionMessageSearchIsBlank(parsed: SessionMessageSearchQuery): boolean {
  return !parsed.text
    && parsed.projects.length === 0
    && parsed.statuses.length === 0
    && parsed.scope === undefined
    && parsed.limit === undefined
}

/** Resolve a `project:` filter value to a project id — the literal id, or a
 * case-insensitive name match. */
export function resolveNamedSearchProject(
  projects: readonly Project[],
  value: string,
): string | undefined {
  if (projects.some((project) => project.id === value)) return value
  const lowered = value.toLowerCase()
  return projects.find((project) => project.name.toLowerCase() === lowered)?.id
}
