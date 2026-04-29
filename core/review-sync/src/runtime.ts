/** Runtime context and output helpers for CLI-compatible command execution. */
import type { ReviewSyncEnv, ReviewSyncResult, RuntimeContext } from "./types.ts"

/** Normalizes optional runtime hooks to concrete values. */
export function createRuntimeContext(env: ReviewSyncEnv) {
  return {
    cwd: env.cwd ?? process.cwd(),
    stdout: env.stdout,
    stderr: env.stderr,
    env: env.env ?? process.env,
  } satisfies RuntimeContext
}

/** Writes command output to stdout for successful states and stderr for hard errors. */
export function writeResult(context: RuntimeContext, result: ReviewSyncResult) {
  const target = result.status === "error" ? context.stderr : context.stdout
  target?.write(`${result.message}\n`)
}
