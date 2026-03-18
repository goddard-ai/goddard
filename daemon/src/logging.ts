import { randomUUID } from "node:crypto"

const defaultWriteLine = (line: string) => {
  process.stdout.write(`${line}\n`)
}

const secretKeys = new Set(["token", "authorization", "goddard_session_token"])
const envSecretFragments = ["TOKEN", "SECRET", "KEY", "AUTH"]

// Function that writes a single serialized daemon log line.
export type DaemonLogWriter = (line: string) => void

// Structured preview emitted when long text must be truncated for logs.
export type DaemonTextPreview = {
  text: string
  byteLength: number
  truncated: boolean
}

// Sanitization settings used for payload and message previews.
export type DaemonSanitizeOptions = {
  maxStringLength?: number
  parentKey?: string
}

export function createDaemonLogger(writeLine: DaemonLogWriter = defaultWriteLine) {
  return {
    log(event: string, fields: Record<string, unknown> = {}) {
      writeLine(
        JSON.stringify({
          scope: "daemon",
          at: new Date().toISOString(),
          event,
          ...fields,
        }),
      )
    },
    createOpId() {
      return randomUUID()
    },
  }
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

export function readSessionIdForLog(value: unknown): string | undefined {
  if (!value || typeof value !== "object") {
    return undefined
  }

  if ("id" in value && typeof value.id === "string") {
    return value.id
  }

  if ("session" in value && value.session && typeof value.session === "object") {
    const session = value.session as { id?: unknown }
    if (typeof session.id === "string") {
      return session.id
    }
  }

  return undefined
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
