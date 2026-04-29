/** Error and result helpers shared by review-sync command modules. */
import type { ReviewSyncCommand, ReviewSyncResult, ReviewSyncStatus } from "./types.ts"

/** User-facing command error with stable status and exit-code metadata. */
export class UserError extends Error {
  readonly status
  readonly exitCode

  constructor(message: string, status: ReviewSyncStatus = "error", exitCode = 1) {
    super(message)
    this.status = status
    this.exitCode = exitCode
  }
}

/** Converts thrown errors into the stable command result contract. */
export function createErrorResult(command: ReviewSyncCommand, error: unknown) {
  const userError = error instanceof UserError ? error : null
  return createReviewSyncResult({
    exitCode: userError?.exitCode ?? 1,
    command,
    status: userError?.status ?? "error",
    message: error instanceof Error ? error.message : String(error),
  })
}

/** Preserves the public result shape for callers even when fields are absent at runtime. */
export function createReviewSyncResult(result: ReviewSyncResult) {
  return result
}
