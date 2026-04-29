/** Runtime context and output helpers for CLI-compatible command execution. */
import type { ReviewSyncResult, RuntimeContext } from "./types.ts"

/** Normalizes the command cwd into the context passed through internal operations. */
export function createRuntimeContext(cwd: string) {
  return {
    cwd,
  } satisfies RuntimeContext
}

/** Writes command output to stdout for successful states and stderr for hard errors. */
export function writeResult(result: ReviewSyncResult) {
  const target = result.status === "error" ? process.stderr : process.stdout
  target.write(`${result.message}\n`)
}
