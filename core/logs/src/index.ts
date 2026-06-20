import { randomBytes } from "node:crypto"
import { mkdirSync } from "node:fs"
import { dirname } from "node:path"
import { getGoddardLogDatabasePath } from "@goddard-ai/paths/node"
import { Database, type SQLQueryBindings } from "bun:sqlite"
import { getErrorMessage, isPlainObject } from "radashi"

export type LogLevel = "debug" | "error" | "info" | "log" | "warn"

export type LogScope = "app" | "daemon" | (string & {})

export type LogProperties = Record<string, unknown>

export type LogCollapsedKind = "array" | "object" | "string"

export type LogCollapsedValue = {
  id: string
  createdAt: string
  kind: LogCollapsedKind
  byteLength: number
  body: unknown
}

export type LogEntry = {
  id: number
  at: string
  scope: LogScope
  level: LogLevel
  pid: number
  message: string
  properties: LogProperties
}

export type LogQuery = {
  afterId?: number
  beforeId?: number
  debugScope?: string
  level?: LogLevel
  limit?: number
  since?: string
  scope?: string
  grep?: string
  regex?: string
  properties?: Record<string, string>
}

export type Logger = {
  debug: (message: string, properties?: LogProperties) => void
  error: (message: string, properties?: LogProperties) => void
  info: (message: string, properties?: LogProperties) => void
  log: (message: string, properties?: LogProperties) => void
  warn: (message: string, properties?: LogProperties) => void
}

export type DebugLogger = (message: string, properties?: LogProperties) => void

export type LogStore = {
  append: (input: {
    at?: Date | string
    scope: LogScope
    level: LogLevel
    pid?: number
    message: string
    properties?: LogProperties
  }) => LogEntry
  close: () => void
  expand: (id: string) => LogCollapsedValue | null
  query: (query?: LogQuery) => LogEntry[]
  retainSince: (since: Date | string) => void
}

const defaultInlineByteLimit = 512
const defaultLimit = 100
const maxLimit = 1000
const logLevelRanks = {
  debug: 0,
  info: 1,
  log: 1,
  warn: 2,
  error: 3,
} satisfies Record<LogLevel, number>
const secretKeys = new Set(["token", "authorization", "goddard_session_token"])
const envSecretFragments = ["TOKEN", "SECRET", "KEY", "AUTH"]
const ulidAlphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

export function createLogStore(options: { databasePath?: string; inlineByteLimit?: number } = {}) {
  const databasePath = options.databasePath ?? getGoddardLogDatabasePath()
  mkdirSync(dirname(databasePath), { recursive: true })
  const db = new Database(databasePath, { create: true })
  db.exec("PRAGMA journal_mode = WAL")
  db.exec("PRAGMA busy_timeout = 5000")
  db.exec(`
    CREATE TABLE IF NOT EXISTS log_entries (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      at TEXT NOT NULL,
      scope TEXT NOT NULL,
      level TEXT NOT NULL,
      pid INTEGER NOT NULL,
      message TEXT NOT NULL,
      properties_json TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS log_collapsed_values (
      id TEXT PRIMARY KEY,
      created_at TEXT NOT NULL,
      kind TEXT NOT NULL,
      byte_length INTEGER NOT NULL,
      body_json TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS log_entries_at_idx ON log_entries(at);
    CREATE INDEX IF NOT EXISTS log_entries_scope_at_idx ON log_entries(scope, at);
    CREATE INDEX IF NOT EXISTS log_entries_message_at_idx ON log_entries(message, at);
  `)

  const inlineByteLimit = options.inlineByteLimit ?? defaultInlineByteLimit

  function collapseValue(value: unknown, parentKey?: string): unknown {
    const redacted = redactValue(value, parentKey)

    if (typeof redacted === "string") {
      return collapseString(redacted)
    }

    if (Array.isArray(redacted)) {
      return collapseJsonValue("array", redacted)
    }

    if (isPlainObject(redacted)) {
      return collapseJsonValue("object", redacted)
    }

    return redacted
  }

  function collapseString(value: string) {
    const byteLength = byteLengthOf(value)
    if (byteLength <= inlineByteLimit) {
      return value
    }

    return insertCollapsedValue("string", value, byteLength)
  }

  function collapseJsonValue(kind: "array" | "object", value: unknown) {
    const serialized = JSON.stringify(value)
    const byteLength = byteLengthOf(serialized)
    if (byteLength <= inlineByteLimit) {
      return value
    }

    return insertCollapsedValue(kind, value, byteLength)
  }

  function insertCollapsedValue(kind: LogCollapsedKind, value: unknown, byteLength: number) {
    const id = `${toCollapsedPrefix(kind)}_${createUlid()}`
    db.query(
      `INSERT INTO log_collapsed_values (id, created_at, kind, byte_length, body_json)
       VALUES (?, ?, ?, ?, ?)`,
    ).run(id, new Date().toISOString(), kind, byteLength, JSON.stringify(value))
    return id
  }

  const store: LogStore = {
    append(input) {
      const at = normalizeDate(input.at ?? new Date())
      const properties = Object.fromEntries(
        Object.entries(input.properties ?? {}).map(([key, value]) => [
          key,
          shouldRedactKey(key, undefined) ? "[redacted]" : collapseValue(value, key),
        ]),
      )
      const result = db
        .query(
          `INSERT INTO log_entries (at, scope, level, pid, message, properties_json)
           VALUES (?, ?, ?, ?, ?, ?)
           RETURNING id`,
        )
        .get(
          at,
          input.scope,
          input.level,
          input.pid ?? process.pid,
          input.message,
          JSON.stringify(properties),
        ) as { id: number }

      return {
        id: result.id,
        at,
        scope: input.scope,
        level: input.level,
        pid: input.pid ?? process.pid,
        message: input.message,
        properties,
      }
    },
    close() {
      db.close()
    },
    expand(id) {
      const row = db
        .query(
          `SELECT id, created_at, kind, byte_length, body_json
           FROM log_collapsed_values
           WHERE id = ?`,
        )
        .get(id) as {
        id: string
        created_at: string
        kind: LogCollapsedKind
        byte_length: number
        body_json: string
      } | null

      return row
        ? {
            id: row.id,
            createdAt: row.created_at,
            kind: row.kind,
            byteLength: row.byte_length,
            body: JSON.parse(row.body_json),
          }
        : null
    },
    query(query = {}) {
      const clauses: string[] = []
      const bindings: SQLQueryBindings[] = []
      const limit = Math.min(Math.max(query.limit ?? defaultLimit, 1), maxLimit)
      const sqlLimit = query.regex ? maxLimit * 20 : limit
      const order = query.beforeId == null ? "ASC" : "DESC"

      if (query.afterId != null) {
        clauses.push("id > ?")
        bindings.push(query.afterId)
      }

      if (query.beforeId != null) {
        clauses.push("id < ?")
        bindings.push(query.beforeId)
      }

      if (query.since) {
        clauses.push("at >= ?")
        bindings.push(query.since)
      }

      if (query.scope) {
        clauses.push("scope = ?")
        bindings.push(query.scope)
      }

      if (query.level) {
        clauses.push(toLogLevelRankSql("level") + " >= ?")
        bindings.push(logLevelRanks[query.level])
      }

      if (query.debugScope) {
        clauses.push("level = ?")
        bindings.push("debug")
        clauses.push(
          `(CAST(json_extract(properties_json, '$."debugScope"') AS TEXT) = ? OR CAST(json_extract(properties_json, '$."debugScope"') AS TEXT) LIKE ?)`,
        )
        bindings.push(query.debugScope, `${query.debugScope}.%`)
      }

      if (query.grep) {
        const pattern = `%${query.grep}%`
        clauses.push("(message LIKE ? OR properties_json LIKE ?)")
        bindings.push(pattern, pattern)
      }

      for (const [key, value] of Object.entries(query.properties ?? {})) {
        clauses.push(`CAST(json_extract(properties_json, ?) AS TEXT) = ?`)
        bindings.push(toJsonPath(key), value)
      }

      bindings.push(sqlLimit)
      const where = clauses.length > 0 ? `WHERE ${clauses.join(" AND ")}` : ""
      const rows = db
        .query(
          `SELECT id, at, scope, level, pid, message, properties_json
           FROM log_entries
           ${where}
           ORDER BY id ${order}
           LIMIT ?`,
        )
        .all(...bindings) as Array<{
        id: number
        at: string
        scope: LogScope
        level: LogLevel
        pid: number
        message: string
        properties_json: string
      }>
      const entries = rows.map((row) => ({
        id: row.id,
        at: row.at,
        scope: row.scope,
        level: row.level,
        pid: row.pid,
        message: row.message,
        properties: JSON.parse(row.properties_json) as LogProperties,
      }))
      const filteredEntries = query.regex
        ? filterByRegex(entries, query.regex).slice(0, limit)
        : entries

      return query.beforeId == null ? filteredEntries : filteredEntries.reverse()
    },
    retainSince(since) {
      const sinceDate = normalizeDate(since)
      db.transaction(() => {
        db.query("DELETE FROM log_entries WHERE at < ?").run(sinceDate)
        db.query(
          `DELETE FROM log_collapsed_values
           WHERE id NOT IN (
             SELECT value
             FROM log_entries, json_each(log_entries.properties_json)
             WHERE json_type(log_entries.properties_json, json_each.fullkey) = 'text'
               AND (value LIKE 'obj_%' OR value LIKE 'arr_%' OR value LIKE 'str_%')
           )`,
        ).run()
      })()
    },
  }

  return store
}

export function createLogger(options: {
  scope: LogScope
  store?: LogStore
  pid?: number
  onLine?: (line: string) => void
}): Logger {
  const store = options.store ?? createLogStore()

  function write(level: LogLevel, message: string, properties: LogProperties = {}) {
    const entry = store.append({
      scope: options.scope,
      level,
      pid: options.pid,
      message,
      properties,
    })
    options.onLine?.(formatLogEntry(entry))
  }

  return {
    debug: (message, properties) => write("debug", message, properties),
    error: (message, properties) => write("error", message, properties),
    info: (message, properties) => write("info", message, properties),
    log: (message, properties) => write("log", message, properties),
    warn: (message, properties) => write("warn", message, properties),
  }
}

export function createDebug(
  debugScope: string,
  options: {
    scope: LogScope
    store?: LogStore
    pid?: number
    onLine?: (line: string) => void
  },
): DebugLogger {
  const logger = createLogger(options)

  return (message, properties = {}) => {
    logger.debug(message, {
      ...properties,
      debugScope,
    })
  }
}

export function formatLogEntry(entry: LogEntry) {
  const properties = Object.entries(entry.properties)
    .filter(([, value]) => value !== undefined)
    .map(([key, value]) => `${key}=${formatPropertyValue(value)}`)

  return [
    entry.id,
    entry.at,
    entry.scope,
    entry.level,
    entry.message,
    `pid=${entry.pid}`,
    ...properties,
  ].join(" ")
}

export function subtractHours(value: Date, hours: number) {
  return new Date(value.getTime() - hours * 60 * 60 * 1000)
}

function redactValue(value: unknown, parentKey?: string): unknown {
  if (typeof value === "string") {
    if (parentKey === "env" && envSecretFragments.some((fragment) => value.includes(fragment))) {
      return "[redacted]"
    }

    return value
  }

  if (Array.isArray(value)) {
    return value.map((item) => redactValue(item, parentKey))
  }

  if (!isPlainObject(value)) {
    return value
  }

  return Object.fromEntries(
    Object.entries(value).map(([key, nestedValue]) => [
      key,
      shouldRedactKey(key, parentKey) ? "[redacted]" : redactValue(nestedValue, key),
    ]),
  )
}

function formatPropertyValue(value: unknown) {
  if (typeof value === "string") {
    if (isCollapsedValueId(value)) {
      return `{${value}}`
    }

    return value.includes(" ") ? JSON.stringify(value) : value
  }

  return JSON.stringify(value)
}

function isCollapsedValueId(value: string) {
  return /^(obj|arr|str)_[0-9A-HJKMNP-TV-Z]{26}$/.test(value)
}

function isSecretKey(key: string) {
  return secretKeys.has(key.toLowerCase())
}

function shouldRedactKey(key: string, parentKey: string | undefined) {
  if (isSecretKey(key)) {
    return true
  }

  const normalizedParentKey = parentKey?.toUpperCase()
  if (normalizedParentKey !== "ENV" && normalizedParentKey?.endsWith("_ENV") !== true) {
    return false
  }

  const uppercaseKey = key.toUpperCase()
  return envSecretFragments.some((fragment) => uppercaseKey.includes(fragment))
}

function filterByRegex(entries: LogEntry[], pattern: string) {
  const regex = new RegExp(pattern)
  return entries.filter((entry) =>
    regex.test(`${entry.message} ${JSON.stringify(entry.properties)}`),
  )
}

function toJsonPath(key: string) {
  return `$."${key.replaceAll('"', '\\"')}"`
}

function toLogLevelRankSql(column: string) {
  return `CASE ${column} WHEN 'debug' THEN 0 WHEN 'info' THEN 1 WHEN 'log' THEN 1 WHEN 'warn' THEN 2 WHEN 'error' THEN 3 ELSE 1 END`
}

function normalizeDate(value: Date | string) {
  return value instanceof Date ? value.toISOString() : value
}

function byteLengthOf(value: string) {
  return new TextEncoder().encode(value).byteLength
}

function toCollapsedPrefix(kind: LogCollapsedKind) {
  if (kind === "array") {
    return "arr"
  }

  if (kind === "string") {
    return "str"
  }

  return "obj"
}

function createUlid() {
  const now = Date.now()
  let time = ""
  let value = now
  for (let index = 0; index < 10; index += 1) {
    time = ulidAlphabet[value % 32] + time
    value = Math.floor(value / 32)
  }

  let random = ""
  for (const byte of randomBytes(16)) {
    random += ulidAlphabet[byte % 32]
  }

  return `${time}${random}`
}

export function toErrorProperties(error: unknown) {
  if (error instanceof Error) {
    return {
      errorMessage: error.message,
      errorName: error.name,
      errorStack: error.stack,
      errorCauseMessage: error.cause === undefined ? undefined : getErrorMessage(error.cause),
    }
  }

  return {
    errorMessage: getErrorMessage(error),
    errorName: typeof error,
  }
}
