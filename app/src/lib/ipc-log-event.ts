import type { IpcClientHookEvent } from "@goddard-ai/ipc"
import { getErrorMessage, isPlainObject } from "radashi"

const secretKeys = new Set(["token", "authorization", "goddard_session_token"])
const envSecretFragments = ["TOKEN", "SECRET", "KEY", "AUTH"]

/** Formats one daemon-client hook event for sanitized durable debug logging. */
export function formatClientIpcLogEvent(event: IpcClientHookEvent) {
  const base = {
    opId: event.opId,
    requestName: event.routeName,
    method: event.method,
    pathPattern: event.pathPattern,
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
        status: event.status,
        response: createPayloadPreview(event.response),
      },
    }
  }

  return {
    message: "ipc.client.request_failed",
    properties: {
      ...base,
      durationMs: event.durationMs,
      status: event.status,
      ...toErrorProperties(event.error),
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

  if (Symbol.asyncIterator in value) {
    return "[AsyncIterable]"
  }

  if (!isPlainObject(value)) {
    return Object.prototype.toString.call(value)
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

function toErrorProperties(error: unknown) {
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
