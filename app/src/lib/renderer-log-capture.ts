import { getErrorMessage } from "radashi"

import type { AppLogInput } from "~/shared/desktop-rpc.ts"

type RendererStructuredLogInput = {
  __goddardLog: true
  message: string
  debugScope?: string
  properties?: Record<string, unknown>
}

type RendererDebugLogInput = {
  __goddardDebugLog: true
  debugScope: string
  message: string
  properties?: Record<string, unknown>
}

type RendererLogCaptureWindow = Window & {
  __goddardDidInstallLogCapture?: boolean
}

export type RendererLogCaptureInput = Omit<AppLogInput, "source">

type RendererLogWriter = (input: RendererLogCaptureInput) => Promise<void> | void

const consoleMethods: AppLogInput["level"][] = ["debug", "error", "info", "log", "warn"]

export function isRendererLogCaptureInstalled() {
  if (typeof window === "undefined") {
    return false
  }

  return Boolean((window as RendererLogCaptureWindow).__goddardDidInstallLogCapture)
}

export function installRendererLogCapture(writeLog: RendererLogWriter) {
  if (typeof window === "undefined") {
    return
  }

  if (isRendererLogCaptureInstalled()) {
    return
  }

  markRendererLogCaptureInstalled()

  for (const method of consoleMethods) {
    const original = console[method].bind(console)
    console[method] = (...args: unknown[]) => {
      const structuredLog = readRendererStructuredLog(args)
      const debugLog = method === "debug" ? readRendererDebugLog(args) : null
      void (structuredLog
        ? writeLog({ level: method, ...structuredLog })
        : debugLog
          ? writeDebugLog(writeLog, debugLog)
          : writeLog({
              level: method,
              message: args.map(formatConsoleValue).join(" "),
            }))
      original(...args)
    }
  }

  window.addEventListener("error", (event) => {
    void writeLog({
      level: "error",
      message: "app.renderer.uncaught_error",
      properties: toErrorProperties(event.error ?? event.message),
    })
  })
  window.addEventListener("unhandledrejection", (event) => {
    void writeLog({
      level: "error",
      message: "app.renderer.unhandled_rejection",
      properties: toErrorProperties(event.reason),
    })
  })
}

/** Emits one structured renderer log through the installed console capture boundary. */
export function writeRendererLog(input: RendererLogCaptureInput) {
  const payload: RendererStructuredLogInput = {
    __goddardLog: true,
    message: input.message,
    debugScope: input.debugScope,
    properties: input.properties,
  }
  console[input.level](payload)
}

/** Emits one scoped renderer debug record. */
export function writeRendererDebug(
  debugScope: string,
  message: string,
  properties?: Record<string, unknown>,
) {
  writeRendererLog({ level: "debug", debugScope, message, properties })
}

/** Emits one renderer error with stable structured error properties. */
export function writeRendererError(
  message: string,
  error: unknown,
  properties?: Record<string, unknown>,
) {
  writeRendererLog({
    level: "error",
    message,
    properties: {
      ...properties,
      ...toErrorProperties(error),
    },
  })
}

function markRendererLogCaptureInstalled() {
  if (typeof window === "undefined") {
    return
  }

  const rendererWindow = window as RendererLogCaptureWindow
  rendererWindow.__goddardDidInstallLogCapture = true
}

function writeDebugLog(writeLog: RendererLogWriter, input: RendererDebugLogInput) {
  return writeLog({
    level: "debug",
    message: input.message,
    debugScope: input.debugScope,
    properties: input.properties,
  })
}

function readRendererStructuredLog(
  args: unknown[],
): Omit<RendererStructuredLogInput, "__goddardLog"> | null {
  const [input] = args
  if (!input || typeof input !== "object") {
    return null
  }

  const record = input as Partial<RendererStructuredLogInput>
  if (record.__goddardLog !== true || typeof record.message !== "string") {
    return null
  }

  return {
    message: record.message,
    debugScope: typeof record.debugScope === "string" ? record.debugScope : undefined,
    properties:
      record.properties && typeof record.properties === "object" ? record.properties : undefined,
  }
}

function readRendererDebugLog(args: unknown[]): RendererDebugLogInput | null {
  const [input] = args
  if (!input || typeof input !== "object") {
    return null
  }

  const record = input as Partial<RendererDebugLogInput>
  if (
    record.__goddardDebugLog !== true ||
    typeof record.debugScope !== "string" ||
    typeof record.message !== "string"
  ) {
    return null
  }

  return {
    __goddardDebugLog: true,
    debugScope: record.debugScope,
    message: record.message,
    properties:
      record.properties && typeof record.properties === "object" ? record.properties : undefined,
  }
}

function formatConsoleValue(value: unknown) {
  if (typeof value === "string") {
    return value
  }

  if (value instanceof Error) {
    return value.stack ?? value.message
  }

  try {
    return JSON.stringify(value)
  } catch {
    return String(value)
  }
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
