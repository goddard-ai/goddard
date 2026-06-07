import { rm } from "node:fs/promises"

const WINDOWS_BUSY_ERROR_CODES = new Set(["EBUSY", "ENOTEMPTY", "EPERM"])

/** Removes temporary test paths after Windows has released recently closed handles. */
export async function removeTemporaryPath(path: string): Promise<void> {
  for (let attempt = 0; attempt < 8; attempt += 1) {
    try {
      await rm(path, { recursive: true, force: true })
      return
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code
      if (process.platform !== "win32" || !code || !WINDOWS_BUSY_ERROR_CODES.has(code)) {
        throw error
      }

      await Bun.sleep(50 * (attempt + 1))
    }
  }

  await rm(path, { recursive: true, force: true })
}
