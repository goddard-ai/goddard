import * as path from "node:path"

const prefix = process.platform === "win32" ? "//./pipe/" : ""

export const ipcPath = {
  prefix,

  /**
   * Return a cross-platform IPC path
   */
  resolve(p: string): string {
    const normalized = path.posix.normalize(p)

    if (prefix.endsWith("/") && normalized.startsWith("/")) {
      return prefix + normalized.slice(1)
    }

    return prefix + normalized
  },
} as const
