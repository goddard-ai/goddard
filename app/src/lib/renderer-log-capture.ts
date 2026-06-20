import type { AppLogInput } from "~/shared/desktop-rpc.ts"

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
      const debugLog = method === "debug" ? readRendererDebugLog(args) : null
      void (debugLog
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
      message: formatErrorEvent(event),
    })
  })
  window.addEventListener("unhandledrejection", (event) => {
    void writeLog({
      level: "error",
      message: formatConsoleValue(event.reason),
    })
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

function formatErrorEvent(event: ErrorEvent) {
  return event.error ? formatConsoleValue(event.error) : event.message
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
