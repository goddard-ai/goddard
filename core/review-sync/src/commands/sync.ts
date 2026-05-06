/** Sync command implementation for review-sync. */
import { command } from "cmd-ts"

import { createReviewSyncResult } from "../errors.ts"
import { createRuntimeContext } from "../runtime.ts"
import { inferSession } from "../session.ts"
import { readSessionState } from "../state.ts"
import { syncSession } from "../sync.ts"
import type { ReviewSyncWorktreeInput, RuntimeContext, SessionState } from "../types.ts"

/** Runs one sync operation for the session inferred from the current worktree. */
export async function syncReviewSession(input: ReviewSyncWorktreeInput) {
  const context = createRuntimeContext(input.cwd)
  const session = await inferSession(context)
  return await syncLoadedReviewSyncSession(session, context)
}

/** Syncs an already selected session without re-inferring ownership from cwd. */
export async function syncLoadedReviewSyncSession(session: SessionState, context: RuntimeContext) {
  const syncResult = await syncSession(session, context)
  const latest = await readSessionState(session)

  return createReviewSyncResult({
    exitCode: 0,
    command: "sync",
    status: syncResult.status === "rejected-human-patch" ? "rejected-human-patch" : "ok",
    sessionId: latest.sessionId,
    reviewBranch: latest.reviewBranch,
    acceptedPatchPath: syncResult.acceptedPatchPath ?? undefined,
    rejectedPatchPath: syncResult.rejectedPatchPath ?? undefined,
    message:
      syncResult.status === "rejected-human-patch"
        ? `Human patch rejected and saved to ${syncResult.rejectedPatchPath}. Review branch refreshed from ${latest.agentBranch}.`
        : `Synced ${latest.agentBranch} to ${latest.reviewBranch}.`,
  })
}

/** Builds the sync subcommand. */
export function createSyncCommand(cwd: string) {
  return command({
    name: "sync",
    description: "Apply clean human edits to the agent worktree and refresh the review branch",
    args: {},
    handler: () => syncReviewSession({ cwd }),
  })
}
