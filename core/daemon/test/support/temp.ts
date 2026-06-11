import { removeTemporaryPath as removeTemporaryPathWithRetries } from "../../../test-support/windows-fixtures.ts"

const WINDOWS_BUSY_RETRY_TIMEOUT_MS = 5_000

/** Removes temporary test paths after Windows has released recently closed handles. */
export async function removeTemporaryPath(path: string): Promise<void> {
  await removeTemporaryPathWithRetries(path, {
    retryTimeoutMs: WINDOWS_BUSY_RETRY_TIMEOUT_MS,
    ignoreBusyAfterTimeout: true,
  })
}
