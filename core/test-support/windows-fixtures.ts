import { spawn, type ChildProcess } from "node:child_process"
import { readFile, rm } from "node:fs/promises"

const WINDOWS_BUSY_ERROR_CODES = new Set(["EBUSY", "ENOTEMPTY", "EPERM"])
const DEFAULT_WINDOWS_BUSY_RETRY_DELAY_MS = 250
const DEFAULT_WINDOWS_BUSY_RETRY_TIMEOUT_MS = 60_000

export function sleep(ms: number) {
  return new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })
}

export function isWindowsBusyError(error: unknown) {
  const code = (error as NodeJS.ErrnoException).code
  return process.platform === "win32" && code != null && WINDOWS_BUSY_ERROR_CODES.has(code)
}

export async function settleWindowsHandles(ms: number) {
  if (process.platform === "win32") {
    await sleep(ms)
  }
}

export async function removeTemporaryPath(
  path: string,
  options: {
    retryDelayMs?: number
    retryTimeoutMs?: number
    ignoreBusyAfterTimeout?: boolean
  } = {},
): Promise<void> {
  const retryDelayMs = options.retryDelayMs ?? DEFAULT_WINDOWS_BUSY_RETRY_DELAY_MS
  const deadline = Date.now() + (options.retryTimeoutMs ?? DEFAULT_WINDOWS_BUSY_RETRY_TIMEOUT_MS)

  while (true) {
    try {
      await rm(path, { recursive: true, force: true })
      return
    } catch (error) {
      if (!isWindowsBusyError(error)) {
        throw error
      }
      if (Date.now() >= deadline) {
        if (options.ignoreBusyAfterTimeout) {
          return
        }
        throw error
      }

      await sleep(retryDelayMs)
    }
  }
}

export async function readTextWhenAvailable(path: string): Promise<string | null> {
  try {
    return await readFile(path, "utf-8")
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code !== "ENOENT" && !isWindowsBusyError(error)) {
      throw error
    }
    return null
  }
}

export async function terminateProcessTree(child: ChildProcess) {
  if (process.platform === "win32" && child.pid) {
    await new Promise<void>((resolve) => {
      const taskkill = spawn("taskkill", ["/pid", String(child.pid), "/t", "/f"], {
        stdio: "ignore",
      })
      taskkill.once("exit", () => resolve())
      taskkill.once("error", () => resolve())
    })
    return
  }

  child.kill("SIGTERM")
}
