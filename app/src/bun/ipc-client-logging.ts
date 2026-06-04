import type { IpcClientHook, IpcClientHookEvent } from "@goddard-ai/ipc"
import { getErrorMessage } from "radashi"

const secretKeys = new Set(["token", "authorization", "goddard_session_token"])
const envSecretFragments = ["TOKEN", "SECRET", "KEY", "AUTH"]

/** Creates the development-only daemon IPC console hook for the desktop host. */
export function createClientIpcLogHook(): IpcClientHook | undefined {
  if (!isClientIpcLoggingEnabled()) {
    return undefined
  }

  return (event) => {
    console.log(formatClientIpcLogEvent(event))
  }
}

function isClientIpcLoggingEnabled() {
  return (
    (process.env.NODE_ENV === "development" || Bun.env.NODE_ENV === "development") &&
    (process.env.GODDARD_CLIENT_IPC_LOG === "1" || Bun.env.GODDARD_CLIENT_IPC_LOG === "1")
  )
}

function formatClientIpcLogEvent(event: IpcClientHookEvent) {
  const base = {
    scope: "client",
    at: new Date().toISOString(),
    event: `ipc.client.${event.type}`,
    opId: event.opId,
    requestName: event.routeName,
  }

  if (event.type === "request.start") {
    return JSON.stringify({
      ...base,
      payload: createPayloadPreview(event.payload),
    })
  }

  if (event.type === "request.success") {
    return JSON.stringify({
      ...base,
      durationMs: event.durationMs,
      response: createPayloadPreview(event.response),
    })
  }

  return JSON.stringify({
    ...base,
    durationMs: event.durationMs,
    errorMessage: getErrorMessage(event.error),
  })
}

function createPayloadPreview(value: unknown): unknown {
  return sanitizeValue(value, 512)
}

function sanitizeValue(value: unknown, maxStringLength: number, parentKey?: string): unknown {
  if (typeof value === "string") {
    return sanitizeString(value, maxStringLength, parentKey)
  }

  if (Array.isArray(value)) {
    return value.map((item) => sanitizeValue(item, maxStringLength, parentKey))
  }

  if (!value || typeof value !== "object") {
    return value
  }

  return Object.fromEntries(
    Object.entries(value).map(([key, nestedValue]) => [
      key,
      isSecretKey(key) ? "[redacted]" : sanitizeValue(nestedValue, maxStringLength, key),
    ]),
  )
}

function sanitizeString(value: string, maxStringLength: number, parentKey?: string) {
  if (parentKey === "env" && envSecretFragments.some((fragment) => value.includes(fragment))) {
    return "[redacted]"
  }

  if (value.length <= maxStringLength) {
    return value
  }

  return {
    text: `${value.slice(0, maxStringLength)}...`,
    byteLength: new TextEncoder().encode(value).byteLength,
    truncated: true,
  }
}

function isSecretKey(key: string) {
  return secretKeys.has(key.toLowerCase())
}
