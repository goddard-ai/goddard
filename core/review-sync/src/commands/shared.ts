/** Shared helpers for CLI-compatible review-sync commands. */
import { getErrorMessage } from "radashi"

import { createReviewSyncResult } from "../errors.ts"

/** Returns a concise user-facing message for thrown values. */
export function formatThrownError(error: unknown) {
  return getErrorMessage(error)
}

/** Converts cmd-ts parse/help exits into the public structured result shape. */
export function createCmdTsResult(error: { config: { exitCode: number; message: string } }) {
  return createReviewSyncResult({
    exitCode: error.config.exitCode,
    command: "status",
    status: error.config.exitCode === 0 ? "ok" : "error",
    message: error.config.message,
  })
}
