import { AsyncContext } from "@b9g/async-context"
import type { DaemonSession } from "@goddard-ai/schema/daemon"
import kleur from "kleur"
import { inspect } from "node:util"
import { omit } from "radashi"

import {
  FeedbackEventContext,
  IpcRequestContext,
  LoopContext,
  SessionContext,
  WorkforceActorContext,
  WorkforceDispatchContext,
} from "./context.ts"

const defaultWriteLine = (_line: string) => {}
const stdoutWriteLine = (line: string) => {
  process.stdout.write(`${line}\n`)
}

const secretKeys = new Set(["token", "authorization", "goddard_session_token"])
const envSecretFragments = ["TOKEN", "SECRET", "KEY", "AUTH"]

/** Function that writes a single serialized daemon log line. */
export type LogWriter = (line: string) => void

/** Supported terminal output modes for daemon logs. */
export type LogMode = "json" | "pretty" | "verbose"

/** Shared daemon logger surface used across daemon runtime code. */
export type DaemonLogger = {
  log: (event: string, fields?: Record<string, unknown>) => void
  snapshot: () => DaemonLogger
}

/** Structured preview emitted when long text must be truncated for logs. */
export type TextPreview = {
  text: string
  byteLength: number
  truncated: boolean
}

/** Sanitization settings used for payload and message previews. */
export type SanitizeOptions = {
  maxStringLength?: number
  parentKey?: string
}

// Structured log entry emitted by the daemon logger.
type LogEntry = {
  scope: "daemon"
  at: string
  event: string
} & Record<string, unknown>

let logMode: LogMode = "pretty"
let logWriter: LogWriter = defaultWriteLine

/** Configures the shared daemon log writer and output mode for the current process. */
export function configureLogging(options: { writeLine?: LogWriter; mode?: LogMode }): () => void {
  const previousMode = logMode
  const previousWriter = logWriter

  logMode = options.mode ?? logMode
  logWriter = options.writeLine ?? stdoutWriteLine

  return () => {
    logMode = previousMode
    logWriter = previousWriter
  }
}

/** Creates a daemon logger that follows the current global output mode. */
export function createLogger(
  writeLine: LogWriter = defaultWriteLine,
  boundSnapshot: InstanceType<typeof AsyncContext.Snapshot> | null = null,
) {
  const logger: DaemonLogger = {
    log(event: string, fields: Record<string, unknown> = {}) {
      const writeEntry = () => {
        const entry: LogEntry = {
          scope: "daemon",
          at: new Date().toISOString(),
          event,
          ...readAmbientLogFields(),
          ...fields,
        }
        const resolvedWriter = writeLine === defaultWriteLine ? logWriter : writeLine
        resolvedWriter(formatLogEntry(entry, logMode))
      }

      if (boundSnapshot) {
        boundSnapshot.run(writeEntry)
        return
      }

      writeEntry()
    },
    snapshot() {
      const nextSnapshot = boundSnapshot
        ? boundSnapshot.run(() => new AsyncContext.Snapshot())
        : new AsyncContext.Snapshot()
      return createLogger(writeLine, nextSnapshot)
    },
  }

  return logger
}

/** Returns true when daemon logs are rendered in expanded verbose mode. */
export function isVerboseLogging(): boolean {
  return logMode === "verbose"
}

export function createPayloadPreview(value: unknown, options: SanitizeOptions = {}): unknown {
  return sanitizeValue(value, options.maxStringLength ?? 512, options.parentKey)
}

export function createChunkPreview(value: Uint8Array): TextPreview {
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

/** Collects the active daemon async-context fields that should be attached to every log line. */
function readAmbientLogFields() {
  const ipcRequest = IpcRequestContext.get()
  const feedbackEvent = FeedbackEventContext.get()
  const workforceDispatch = WorkforceDispatchContext.get()
  const loop = LoopContext.get()
  const session = SessionContext.get()
  const workforceActor = WorkforceActorContext.get()

  return {
    ipcRequest: ipcRequest && omit(ipcRequest, ["setSessionId"]),
    feedbackEvent,
    workforceDispatch,
    loop,
    session,
    workforceActor,
  }
}

function formatLogEntry(entry: LogEntry, mode: LogMode): string {
  if (mode === "pretty") {
    return formatPrettyLogEntry(entry)
  }

  if (mode === "verbose") {
    return formatVerboseLogEntry(entry)
  }

  return JSON.stringify(entry)
}

function formatPrettyLogEntry(entry: LogEntry): string {
  const fields = Object.entries(entry).filter(([key]) => isMetadataField(key) === false)
  const inlineFields = fields.flatMap(([key, value]) => formatInlineFields(key, value))

  return [kleur.dim(formatTimestamp(entry.at)), kleur.cyan(entry.event), ...inlineFields].join(" ")
}

function formatVerboseLogEntry(entry: LogEntry): string {
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

function formatInlineFields(key: string, value: unknown): string[] {
  if (isPlainObject(value)) {
    const nestedFields = Object.entries(value)
      .map(([nestedKey, nestedValue]) => formatInlineField(`${key}.${nestedKey}`, nestedValue))
      .filter((field) => field !== null)

    if (nestedFields.length > 0) {
      return nestedFields
    }
  }

  const field = formatInlineField(key, value)
  return field ? [field] : []
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

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return false
  }

  const prototype = Object.getPrototypeOf(value)
  return prototype === Object.prototype || prototype === null
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

function truncateText(value: string, maxStringLength: number): string | TextPreview {
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
