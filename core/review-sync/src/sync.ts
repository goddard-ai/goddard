/** Review-sync orchestration for one complete sync cycle. */
import { UserError } from "./errors.ts"
import {
  assertSupportedGitState,
  git,
  resolveCurrentBranch,
  resolveRef,
  resolveRequiredGitCommonDir,
  updateRef,
} from "./git.ts"
import { withSessionLock } from "./lock.ts"
import { handleHumanPatch } from "./patch-flow.ts"
import { validateSessionWorktrees } from "./session.ts"
import { createSnapshotCommit, diffCommits } from "./snapshot.ts"
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

/**
 * Refreshes only the review worktree from the target branch ref while the agent
 * checkout is unavailable.
 */
export async function refreshReviewWorktreeFromAgentBranchRef(
  session: SessionState,
  context: RuntimeContext,
) {
  return await withSessionLock(session, async () => {
    const latest = await readSessionState(session)
    if (latest.paused) {
      return { status: "skipped", reason: "paused" } as const
    }

    await validateReviewWorktreeForRefresh(latest, context)
    const branchHead = await resolveRef(
      latest.reviewWorktree,
      `refs/heads/${latest.agentBranch}`,
      context,
    )
    const renderedSnapshot = await resolveRef(
      latest.reviewWorktree,
      latest.refs.renderedSnapshot,
      context,
    )
    if (!branchHead || !renderedSnapshot || branchHead === renderedSnapshot) {
      return {
        status: "skipped",
        reason: !branchHead
          ? "missing-agent-branch-ref"
          : !renderedSnapshot
            ? "missing-rendered-snapshot"
            : "unchanged",
      } as const
    }

    const reviewSnapshot = await createSnapshotCommit({
      cwd: latest.reviewWorktree,
      label: `${latest.sessionId}:review`,
      context,
    })
    const humanPatch = await diffCommits(
      latest.reviewWorktree,
      renderedSnapshot,
      reviewSnapshot,
      context,
    )
    if (humanPatch.trim()) {
      return { status: "skipped", reason: "pending-human-patch" } as const
    }

    await refreshReviewWorktree(latest, branchHead, context)
    await updateRef(latest.reviewWorktree, latest.refs.renderedSnapshot, branchHead, context)
    latest.updatedAt = new Date().toISOString()
    await writeSessionState(latest)
    await appendEvent(latest, {
      command: "sync",
      status: "synced",
      source: "agent-branch-ref",
    })
    return { status: "refreshed" } as const
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

/** Validates only the worktree that the branch-ref refresh mutates. */
async function validateReviewWorktreeForRefresh(session: SessionState, context: RuntimeContext) {
  await assertSupportedGitState(session.reviewWorktree, context)

  const reviewBranch = await resolveCurrentBranch(session.reviewWorktree, context)
  if (reviewBranch !== session.reviewBranch) {
    throw new UserError(
      `Review worktree must be on ${session.reviewBranch}; currently ${reviewBranch ?? "detached HEAD"}.`,
    )
  }

  const reviewCommonDir = await resolveRequiredGitCommonDir(session.reviewWorktree, context)
  if (reviewCommonDir !== session.repoCommonDir) {
    throw new UserError("Review worktree no longer shares the recorded Git common dir.")
  }
}
