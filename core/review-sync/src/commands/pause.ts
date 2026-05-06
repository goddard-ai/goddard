/** Pause command implementation for review-sync. */
import { command } from "cmd-ts"

import { createReviewSyncResult } from "../errors.ts"
import { withSessionLock } from "../lock.ts"
import { createRuntimeContext } from "../runtime.ts"
import { inferSession } from "../session.ts"
import { appendEvent, readSessionState, writeSessionState } from "../state.ts"
import type { ReviewSyncWorktreeInput, SessionState } from "../types.ts"
import { runCommandSafely } from "./shared.ts"

/** Marks the inferred session paused so later sync commands refuse to mutate it. */
export async function pauseReviewSession(input: ReviewSyncWorktreeInput) {
  const context = createRuntimeContext(input.cwd)
  const session = await inferSession(context)
  const latest = await pauseSession(session)

  return createReviewSyncResult({
    exitCode: 0,
    command: "pause",
    status: "paused",
    sessionId: latest.sessionId,
    reviewBranch: latest.reviewBranch,
    message: `Paused review sync for ${latest.reviewBranch}.`,
  })
}

/** Marks a known session paused without relying on branch-based session inference. */
export async function pauseSession(session: SessionState) {
  return await withSessionLock(session, async () => {
    const latest = await readSessionState(session)
    latest.paused = true
    latest.updatedAt = new Date().toISOString()
    latest.lastSync = {
      status: "paused",
      acceptedPatch: null,
      rejectedPatch: null,
    }
    await writeSessionState(latest)
    await appendEvent(latest, {
      command: "pause",
      status: "paused",
    })
    return latest
  })
}

/** Builds the pause subcommand. */
export function createPauseCommand(cwd: string) {
  return command({
    name: "pause",
    description: "Pause future sync mutations for the inferred session",
    args: {},
    handler: () => runCommandSafely("pause", () => pauseReviewSession({ cwd })),
  })
}
