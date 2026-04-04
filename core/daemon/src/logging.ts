import type { DaemonSession } from "@goddard-ai/schema/daemon"
import kleur from "kleur"
import { randomUUID } from "node:crypto"
import { inspect } from "node:util"

const defaultWriteLine = (_line: string) => {}
const stdoutWriteLine = (line: string) => {
  process.stdout.write(`${line}\n`)
}

const secretKeys = new Set(["token", "authorization", "goddard_session_token"])
const envSecretFragments = ["TOKEN", "SECRET", "KEY", "AUTH"]

/** Function that writes a single serialized daemon log line. */
export type DaemonLogWriter = (line: string) => void

/** Supported terminal output modes for daemon logs. */
export type DaemonLogMode = "json" | "pretty" | "verbose"

/** Structured preview emitted when long text must be truncated for logs. */
export type DaemonTextPreview = {
  text: string
  byteLength: number
  truncated: boolean
}

/** Sanitization settings used for payload and message previews. */
export type DaemonSanitizeOptions = {
  maxStringLength?: number
  parentKey?: string
}

// Structured log entry emitted by the daemon logger.
type DaemonLogEntry = {
  scope: "daemon"
  at: string
  event: string
} & Record<string, unknown>

let daemonLogMode: DaemonLogMode = "pretty"
let daemonLogWriter: DaemonLogWriter = defaultWriteLine

/** Configures the shared daemon log writer and output mode for the current process. */
export function configureDaemonLogging(options: {
  writeLine?: DaemonLogWriter
  mode?: DaemonLogMode
}): () => void {
  const previousMode = daemonLogMode
  const previousWriter = daemonLogWriter

  daemonLogMode = options.mode ?? daemonLogMode
  daemonLogWriter = options.writeLine ?? stdoutWriteLine

  return () => {
    daemonLogMode = previousMode
    daemonLogWriter = previousWriter
  }
}

/** Creates a daemon logger that follows the current global output mode. */
export function createDaemonLogger(writeLine: DaemonLogWriter = defaultWriteLine) {
  return {
    log(event: string, fields: Record<string, unknown> = {}) {
      const entry: DaemonLogEntry = {
        scope: "daemon",
        at: new Date().toISOString(),
        event,
        ...fields,
      }
      const resolvedWriter = writeLine === defaultWriteLine ? daemonLogWriter : writeLine
      resolvedWriter(formatDaemonLogEntry(entry, daemonLogMode))
    },
    createOpId() {
      return randomUUID()
    },
  }
}

/** Returns true when daemon logs are rendered in expanded verbose mode. */
export function isVerboseDaemonLogging(): boolean {
  return daemonLogMode === "verbose"
}

export function createPayloadPreview(value: unknown, options: DaemonSanitizeOptions = {}): unknown {
  return sanitizeValue(value, options.maxStringLength ?? 512, options.parentKey)
}

export function createChunkPreview(value: Uint8Array): DaemonTextPreview {
  const text = new TextDecoder().decode(value)
  const byteLength = Buffer.byteLength(text)
  if (text.length <= 256) {
    return {
      text,
      byteLength,
      truncated: false,
    }
  }

  return {
    text: `${text.slice(0, 256)}...`,
    byteLength,
    truncated: true,
  }
}

export function readSessionIdForLog(value: unknown): DaemonSession["id"] | undefined {
  if (!value || typeof value !== "object") {
    return undefined
  }

  if ("id" in value && typeof value.id === "string" && value.id.startsWith("ses_")) {
    return value.id as DaemonSession["id"]
  }

  if ("session" in value && value.session && typeof value.session === "object") {
    const session = value.session as { id?: unknown }
    if (typeof session.id === "string" && session.id.startsWith("ses_")) {
      return session.id as DaemonSession["id"]
    }
  }

  return undefined
}

function formatDaemonLogEntry(entry: DaemonLogEntry, mode: DaemonLogMode): string {
  if (mode === "pretty") {
    return formatPrettyDaemonLogEntry(entry)
  }

  if (mode === "verbose") {
    return formatVerboseDaemonLogEntry(entry)
  }

  return JSON.stringify(entry)
}

function formatPrettyDaemonLogEntry(entry: DaemonLogEntry): string {
  const fields = Object.entries(entry).filter(([key]) => isMetadataField(key) === false)
  const inlineFields = fields
    .map(([key, value]) => formatInlineField(key, value))
    .filter((value) => value !== null)

  return [kleur.dim(formatTimestamp(entry.at)), kleur.cyan(entry.event), ...inlineFields].join(" ")
}

function formatVerboseDaemonLogEntry(entry: DaemonLogEntry): string {
  const fields = Object.entries(entry).filter(([key]) => isMetadataField(key) === false)
  if (fields.length === 0) {
    return `${kleur.dim(formatTimestamp(entry.at))} ${kleur.bold().cyan(entry.event)}`
  }

  return [
    `${kleur.dim(formatTimestamp(entry.at))} ${kleur.bold().cyan(entry.event)}`,
    ...fields.map(([key, value]) => `${kleur.gray(`  ${key}:`)} ${formatVerboseValue(value)}`),
  ].join("\n")
}

function formatTimestamp(value: string): string {
  return value.replace("T", " ").replace("Z", " UTC")
}

function isMetadataField(key: string): boolean {
  return key === "scope" || key === "at" || key === "event"
}

function formatInlineField(key: string, value: unknown): string | null {
  if (value === undefined) {
    return null
  }

  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return `${kleur.gray(`${key}=`)}${String(value)}`
  }

  if (value === null) {
    return `${kleur.gray(`${key}=`)}null`
  }

  return `${kleur.gray(`${key}=`)}${truncateInlineValue(value)}`
}

function truncateInlineValue(value: unknown): string {
  const serialized = inspect(value, {
    depth: 2,
    colors: false,
    compact: true,
    breakLength: 120,
  })
  return serialized.length <= 96 ? serialized : `${serialized.slice(0, 93)}...`
}

function formatVerboseValue(value: unknown): string {
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return String(value)
  }

  if (value === null || value === undefined) {
    return String(value)
  }

  return inspect(value, {
    depth: 6,
    colors: false,
    compact: false,
    breakLength: 100,
  }).replace(/\n/g, "\n    ")
}

function sanitizeValue(
  value: unknown,
  maxStringLength: number,
  parentKey: string | undefined,
  seen = new WeakSet<object>(),
): unknown {
  if (typeof value === "string") {
    return truncateText(value, maxStringLength)
  }

  if (
    value === null ||
    typeof value === "number" ||
    typeof value === "boolean" ||
    typeof value === "undefined"
  ) {
    return value
  }

  if (typeof value === "bigint") {
    return value.toString()
  }

  if (value instanceof Date) {
    return value.toISOString()
  }

  if (value instanceof Uint8Array) {
    return {
      byteLength: value.byteLength,
    }
  }

  if (value instanceof Error) {
    return {
      name: value.name,
      message: truncateText(value.message, maxStringLength),
      stack: value.stack ? truncateText(value.stack, maxStringLength) : undefined,
    }
  }

  if (Array.isArray(value)) {
    return value.map((entry) => sanitizeValue(entry, maxStringLength, parentKey, seen))
  }

  if (typeof value === "object") {
    if (seen.has(value)) {
      return "[Circular]"
    }

    seen.add(value)

    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => {
        if (shouldRedactKey(key, parentKey)) {
          return [key, "[REDACTED]"]
        }

        return [key, sanitizeValue(entry, maxStringLength, key, seen)]
      }),
    )
  }

  return String(value)
}

function truncateText(value: string, maxStringLength: number): string | DaemonTextPreview {
  const byteLength = Buffer.byteLength(value)
  if (value.length <= maxStringLength) {
    return value
  }

  return {
    text: `${value.slice(0, maxStringLength)}...`,
    byteLength,
    truncated: true,
  }
}

function shouldRedactKey(key: string, parentKey: string | undefined): boolean {
  const normalizedKey = key.toLowerCase()
  if (secretKeys.has(normalizedKey)) {
    return true
  }

  const normalizedParentKey = parentKey?.toUpperCase()
  if (normalizedParentKey !== "ENV" && normalizedParentKey?.endsWith("_ENV") !== true) {
    return false
  }

  const uppercaseKey = key.toUpperCase()
  return envSecretFragments.some((fragment) => uppercaseKey.includes(fragment))
}
