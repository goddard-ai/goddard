import type { IpcClientHook, IpcClientHookEvent } from "@goddard-ai/ipc"
import { getErrorMessage } from "radashi"

import { getAppDebug } from "./logging.ts"

const secretKeys = new Set(["token", "authorization", "goddard_session_token"])
const envSecretFragments = ["TOKEN", "SECRET", "KEY", "AUTH"]

/** Creates the daemon IPC debug hook for the desktop host. */
export function createClientIpcLogHook(): IpcClientHook {
  const debug = getAppDebug("ipc.client")

  return (event) => {
    const { message, properties } = formatClientIpcLogEvent(event)
    debug(message, properties)
  }
}

function formatClientIpcLogEvent(event: IpcClientHookEvent) {
  const base = {
    opId: event.opId,
    requestName: event.routeName,
    method: event.routeName,
  }

  if (event.type === "request.start") {
    return {
      message: "ipc.client.request_started",
      properties: {
        ...base,
        payload: createPayloadPreview(event.payload),
      },
    }
  }

  if (event.type === "request.success") {
    return {
      message: "ipc.client.request_succeeded",
      properties: {
        ...base,
        durationMs: event.durationMs,
        response: createPayloadPreview(event.response),
      },
    }
  }

  return {
    message: "ipc.client.request_failed",
    properties: {
      ...base,
      durationMs: event.durationMs,
      errorMessage: getErrorMessage(event.error),
    },
  }
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
