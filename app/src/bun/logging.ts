import { createLogger, createLogStore, type Logger } from "@goddard-ai/logs"

import type { AppLogInput } from "~/shared/desktop-rpc.ts"

const consoleMethods: AppLogInput["level"][] = ["debug", "error", "info", "log", "warn"]
let appLogger: Logger | undefined

/** Returns the process-global app logger used by the Bun host. */
export function getAppLogger() {
  appLogger ??= createLogger({
    scope: "app",
    store: createLogStore(),
    pid: process.pid,
  })

  return appLogger
}

/** Captures Bun-host console output into the shared SQLite log store for agent inspection. */
export function installAppLogCapture() {
  getAppLogger()

  for (const method of consoleMethods) {
    const original = console[method].bind(console)
    console[method] = (...args: unknown[]) => {
      writeAppLog({
        source: "host",
        level: method,
        message: args.map(formatConsoleValue).join(" "),
      })
      original(...args)
    }
  }
}

/** Appends one normalized app log record into the shared SQLite log store. */
export function writeAppLog(input: AppLogInput) {
  getAppLogger()[input.level](input.message, {
    source: input.source,
    webviewId: input.webviewId,
  })
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
