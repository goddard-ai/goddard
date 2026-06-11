import { rm } from "node:fs/promises"

const WINDOWS_BUSY_ERROR_CODES = new Set(["EBUSY", "ENOTEMPTY", "EPERM"])
const WINDOWS_BUSY_RETRY_DELAY_MS = 250
const WINDOWS_BUSY_RETRY_TIMEOUT_MS = 60_000

/** Removes temporary test paths after Windows has released recently closed handles. */
export async function removeTemporaryPath(path: string): Promise<void> {
  const deadline = Date.now() + WINDOWS_BUSY_RETRY_TIMEOUT_MS
  while (true) {
    try {
      await rm(path, { recursive: true, force: true })
      return
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code
      if (process.platform !== "win32" || !code || !WINDOWS_BUSY_ERROR_CODES.has(code)) {
        throw error
      }
      if (Date.now() >= deadline) {
        throw error
      }

      await Bun.sleep(WINDOWS_BUSY_RETRY_DELAY_MS)
    }
  }
}
