/** Review-sync orchestration for one complete sync cycle. */
import { UserError } from "./errors.ts"
import { git, updateRef } from "./git.ts"
import { withSessionLock } from "./lock.ts"
import { handleHumanPatch } from "./patch-flow.ts"
import { validateSessionWorktrees } from "./session.ts"
import { createSnapshotCommit } from "./snapshot.ts"
import { appendEvent, readSessionState, writeSessionState } from "./state.ts"
import type { RuntimeContext, SessionState } from "./types.ts"

/** Coordinates one complete patch-acceptance and review-refresh cycle. */
export async function syncSession(session: SessionState, context: RuntimeContext) {
  return await withSessionLock(session, async () => {
    const latest = await readSessionState(session)
    if (latest.paused) {
      await appendEvent(latest, {
        command: "sync",
        status: "paused",
      })
      throw new UserError(`Review sync session ${latest.sessionId} is paused.`, "paused", 0)
    }

    await validateSessionWorktrees(latest, context)
    const patchResult = await handleHumanPatch(latest, context)
    const agentSnapshot = await createSnapshotCommit({
      cwd: latest.agentWorktree,
      label: `${latest.sessionId}:agent`,
      context,
    })

    await updateRef(latest.agentWorktree, latest.refs.agentSnapshot, agentSnapshot, context)
    await refreshReviewWorktree(latest, agentSnapshot, context)
    await updateRef(latest.agentWorktree, latest.refs.renderedSnapshot, agentSnapshot, context)

    latest.updatedAt = new Date().toISOString()
    latest.lastSync = {
      status: patchResult.status,
      acceptedPatch: patchResult.acceptedPatchPath,
      rejectedPatch: patchResult.rejectedPatchPath,
    }
    await writeSessionState(latest)
    await appendEvent(latest, {
      command: "sync",
      status: patchResult.status,
      acceptedPatchPath: patchResult.acceptedPatchPath,
      rejectedPatchPath: patchResult.rejectedPatchPath,
    })
    return patchResult
  })
}

/** Moves the checked-out review branch to the latest agent snapshot and cleans mirror state. */
async function refreshReviewWorktree(
  session: SessionState,
  agentSnapshot: string,
  context: RuntimeContext,
) {
  await git(session.reviewWorktree, ["reset", "--hard", agentSnapshot], context)
  await git(session.reviewWorktree, ["clean", "-fd"], context)
}
