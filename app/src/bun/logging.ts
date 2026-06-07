import { createWriteStream } from "node:fs"
import { mkdir } from "node:fs/promises"
import { join } from "node:path"
import { getGoddardTempLogDir } from "@goddard-ai/paths/node"

import type { AppLogInput } from "~/shared/desktop-rpc.ts"

const consoleMethods: AppLogInput["level"][] = ["debug", "error", "info", "log", "warn"]
let appLogStream: Promise<ReturnType<typeof createWriteStream>> | undefined

/** Tees Bun-host console output into the well-known temp log directory for agent inspection. */
export async function installAppLogCapture() {
  await getAppLogStream()

  for (const method of consoleMethods) {
    const original = console[method].bind(console)
    console[method] = (...args: unknown[]) => {
      void writeAppLog({
        source: "host",
        level: method,
        message: args.map(formatConsoleValue).join(" "),
      })
      original(...args)
    }
  }
}

/** Appends one normalized app log record into the temp app log file. */
export async function writeAppLog(input: AppLogInput) {
  const stream = await getAppLogStream()
  stream.write(`${JSON.stringify({ scope: "app", at: new Date().toISOString(), ...input })}\n`)
}

async function getAppLogStream() {
  appLogStream ??= openAppLogStream()
  return await appLogStream
}

async function openAppLogStream() {
  const logDir = getGoddardTempLogDir()
  await mkdir(logDir, { recursive: true })
  return createWriteStream(join(logDir, "app.log"), { flags: "a" })
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
