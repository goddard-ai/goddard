/** Review-sync orchestration for one complete sync cycle. */
import { UserError } from "./errors.ts"
import {
  assertSupportedGitState,
  git,
  isWorktreeClean,
  resolveCurrentBranch,
  resolveRef,
  resolveRequiredGitCommonDir,
  updateRef,
} from "./git.ts"
import { withSessionLock } from "./lock.ts"
import { handleHumanPatch, hasHumanPatch } from "./patch-flow.ts"
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
    let agentSnapshot = await createSnapshotCommit({
      cwd: latest.agentWorktree,
      label: `${latest.sessionId}:agent`,
      context,
    })
    let agentBranchHead = await resolveRef(
      latest.agentWorktree,
      `refs/heads/${latest.agentBranch}`,
      context,
    )
    if (!agentBranchHead) {
      throw new UserError(`Agent branch ${latest.agentBranch} no longer exists.`)
    }
    const acceptedReviewHead = await resolveCleanReviewHeadMatchingAgentSnapshot({
      session: latest,
      agentSnapshot,
      context,
    })
    if (acceptedReviewHead && acceptedReviewHead !== agentBranchHead) {
      await promoteReviewHeadToAgentBranch(latest, acceptedReviewHead, context)
      agentBranchHead = acceptedReviewHead
      agentSnapshot = acceptedReviewHead
    }
    const renderTarget = acceptedReviewHead
      ? {
          branchHead: acceptedReviewHead,
          content: acceptedReviewHead,
          renderedSnapshot: acceptedReviewHead,
        }
      : (await isWorktreeClean(latest.agentWorktree, context))
        ? {
            branchHead: agentBranchHead,
            content: agentBranchHead,
            renderedSnapshot: agentBranchHead,
          }
        : {
            branchHead: agentBranchHead,
            content: agentSnapshot,
            renderedSnapshot: agentSnapshot,
          }

    await updateRef(latest.agentWorktree, latest.refs.agentSnapshot, agentSnapshot, context)
    await renderReviewWorktreeTarget(latest, renderTarget, context)
    await updateRef(
      latest.agentWorktree,
      latest.refs.renderedSnapshot,
      renderTarget.renderedSnapshot,
      context,
    )

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

/** Finds a clean review commit whose tree exactly matches the synchronized agent tree. */
async function resolveCleanReviewHeadMatchingAgentSnapshot(input: {
  session: SessionState
  agentSnapshot: string
  context: RuntimeContext
}) {
  if (!(await isWorktreeClean(input.session.reviewWorktree, input.context))) {
    return null
  }

  const reviewHead = await resolveRef(input.session.reviewWorktree, "HEAD", input.context)
  if (!reviewHead) {
    return null
  }

  const diff = await git(
    input.session.reviewWorktree,
    ["diff", "--quiet", input.agentSnapshot, reviewHead],
    input.context,
    {
      allowFailure: true,
    },
  )
  if (diff.status === 0) {
    return reviewHead
  }
  if (diff.status === 1) {
    return null
  }

  throw new Error(
    `git diff --quiet failed in ${input.session.reviewWorktree}: ${
      diff.stderr.trim() || diff.stdout.trim() || "unknown Git error"
    }`,
  )
}

/** Moves the checked-out agent branch to a content-equivalent review commit. */
async function promoteReviewHeadToAgentBranch(
  session: SessionState,
  reviewHead: string,
  context: RuntimeContext,
) {
  const agentBranch = await resolveCurrentBranch(session.agentWorktree, context)
  if (agentBranch !== session.agentBranch) {
    throw new UserError(
      `Agent worktree must be on ${session.agentBranch}; currently ${agentBranch ?? "detached HEAD"}.`,
    )
  }

  await git(session.agentWorktree, ["reset", "--mixed", reviewHead], context)
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

    if (await hasHumanPatch(latest, context)) {
      return { status: "skipped", reason: "pending-human-patch" } as const
    }

    await renderReviewWorktreeTarget(
      latest,
      {
        branchHead,
        content: branchHead,
        renderedSnapshot: branchHead,
      },
      context,
    )
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

/** Content source that determines whether review HEAD can move during render. */
type ReviewWorktreeRenderTarget = {
  branchHead: string
  content: string
  renderedSnapshot: string
}

/** Renders synchronized content without moving review HEAD to synthetic snapshots. */
async function renderReviewWorktreeTarget(
  session: SessionState,
  target: ReviewWorktreeRenderTarget,
  context: RuntimeContext,
) {
  await git(session.reviewWorktree, ["reset", "--hard", target.branchHead], context)
  if (target.content !== target.branchHead) {
    await git(session.reviewWorktree, ["read-tree", "--reset", "-u", target.content], context)
  }
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
