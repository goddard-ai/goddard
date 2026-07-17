import {
  createDebug,
  createLogger,
  createLogStore,
  toErrorProperties,
  type DebugLogger,
  type Logger,
  type LogStore,
} from "@goddard-ai/logs"
import { getErrorMessage } from "radashi"

import type { AppLogInput } from "~/shared/desktop-rpc.ts"

const consoleMethods: AppLogInput["level"][] = ["debug", "error", "info", "log", "warn"]
let appLogger: Logger | undefined
let appLogStore: LogStore | undefined
const appDebugLoggers = new Map<string, DebugLogger>()
let didInstallLogCapture = false
let didInstallFatalErrorCapture = false

/** Returns the process-global app logger used by the Bun host. */
export function getAppLogger() {
  appLogger ??= createLogger({
    scope: "app",
    store: getAppLogStore(),
    pid: process.pid,
  })

  return appLogger
}

/** Returns a process-global app debug logger for a focused runtime subsystem. */
export function getAppDebug(debugScope: string) {
  let debug = appDebugLoggers.get(debugScope)
  if (!debug) {
    debug = createDebug(debugScope, {
      scope: "app",
      store: getAppLogStore(),
      pid: process.pid,
    })
    appDebugLoggers.set(debugScope, debug)
  }

  return debug
}

/** Captures Bun-host console output into the shared SQLite log store for agent inspection. */
export function installAppLogCapture() {
  if (didInstallLogCapture) {
    return
  }

  getAppLogger()
  didInstallLogCapture = true

  for (const method of consoleMethods) {
    const original = console[method].bind(console)
    console[method] = (...args: unknown[]) => {
      try {
        writeAppLog({
          source: "host",
          level: method,
          message: args.map(formatConsoleValue).join(" "),
        })
      } catch {
        // Console output must stay available when durable logging is unavailable.
      }
      original(...args)
    }
  }
}

/** Captures Bun-host process failures that escape normal app startup and RPC handlers. */
export function installAppFatalErrorCapture() {
  if (didInstallFatalErrorCapture) {
    return
  }

  didInstallFatalErrorCapture = true

  process.on("uncaughtException", (error) => {
    writeAppError("app.host.uncaught_exception", error)
    process.exit(1)
  })

  process.on("unhandledRejection", (reason) => {
    writeAppError("app.host.unhandled_rejection", reason)
    process.exit(1)
  })
}

/** Appends one normalized app log record into the shared SQLite log store. */
export function writeAppLog(input: AppLogInput) {
  if (input.debugScope) {
    getAppDebug(input.debugScope)(input.message, {
      source: input.source,
      webviewId: input.webviewId,
      ...input.properties,
    })
    return
  }

  getAppLogger()[input.level](input.message, {
    source: input.source,
    webviewId: input.webviewId,
    ...input.properties,
  })
}

/** Appends one structured app-host error entry into the shared SQLite log store. */
export function writeAppError(
  message: string,
  error: unknown,
  properties: Record<string, unknown> = {},
) {
  try {
    getAppLogger().error(message, {
      source: "host",
      ...properties,
      ...toErrorProperties(error),
    })
  } catch (loggingError) {
    process.stderr.write(
      `${message}: ${getErrorMessage(error)}\nlogging failed: ${getErrorMessage(loggingError)}\n`,
    )
  }
}

function formatConsoleValue(value: unknown) {
  if (typeof value === "string") {
    return value
  }

  if (value instanceof Error) {
    return value.stack ?? value.message
  }

  return Bun.inspect(value)
}

function getAppLogStore() {
  appLogStore ??= createLogStore()
  return appLogStore
}
