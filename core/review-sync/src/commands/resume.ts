/** Resume command implementation for review-sync. */
import { command } from "cmd-ts"

import { createReviewSyncResult } from "../errors.ts"
import { withSessionLock } from "../lock.ts"
import { createRuntimeContext } from "../runtime.ts"
import { inferSession } from "../session.ts"
import { appendEvent, readSessionState, writeSessionState } from "../state.ts"
import type { ReviewSyncWorktreeInput, RuntimeContext, SessionState } from "../types.ts"
import { runCommandSafely } from "./shared.ts"

/** Clears the paused flag without running an implicit sync. */
export async function resumeReviewSession(input: ReviewSyncWorktreeInput) {
  return await runCommandSafely("resume", () =>
    resumeReviewSessionOperation(createRuntimeContext(input.cwd)),
  )
}

/** Clears paused state for commands that reactivate a saved session. */
export async function resumeSession(session: SessionState, source?: "start" | "watch") {
  return await withSessionLock(session, async () => {
    const latest = await readSessionState(session)
    if (!latest.paused) {
      return latest
    }

    latest.paused = false
    latest.updatedAt = new Date().toISOString()
    await writeSessionState(latest)
    await appendEvent(latest, {
      command: "resume",
      status: "ok",
      ...(source ? { source } : {}),
    })
    return latest
  })
}

/** Performs the resume workflow after CLI parsing and command-level error handling. */
async function resumeReviewSessionOperation(context: RuntimeContext) {
  const session = await inferSession(context)
  const latest = await resumeSession(session)

  return createReviewSyncResult({
    exitCode: 0,
    command: "resume",
    status: "ok",
    sessionId: latest.sessionId,
    reviewBranch: latest.reviewBranch,
    message: `Resumed review sync for ${latest.reviewBranch}.`,
  })
}

/** Builds the resume subcommand. */
export function createResumeCommand(cwd: string) {
  return command({
    name: "resume",
    description: "Resume sync mutations without running an immediate sync",
    args: {},
    handler: () => resumeReviewSession({ cwd }),
  })
}
