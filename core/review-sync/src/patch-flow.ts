/** Human patch acceptance and rejection flow. */
import { UserError } from "./errors.ts"
import { git, resolveRef } from "./git.ts"
import { createSnapshotCommit, diffCommits } from "./snapshot.ts"
import { savePatch } from "./state.ts"
import type { PatchFlowResult, RuntimeContext, SessionState } from "./types.ts"

/** Computes the human patch from the rendered baseline to the review worktree. */
async function createHumanPatch(session: SessionState, context: RuntimeContext) {
  const renderedSnapshot = await resolveRef(
    session.agentWorktree,
    session.refs.renderedSnapshot,
    context,
  )

  if (!renderedSnapshot) {
    return null
  }

  const reviewSnapshot = await createSnapshotCommit({
    cwd: session.reviewWorktree,
    label: `${session.sessionId}:review`,
    context,
  })
  return await diffCommits(session.agentWorktree, renderedSnapshot, reviewSnapshot, context)
}

/**
 * Checks whether the review worktree contains changes beyond the rendered baseline.
 * Avoid this when the caller also needs the patch body; call createHumanPatch
 * once and check for a null or empty patch instead.
 */
export async function hasHumanPatch(session: SessionState, context: RuntimeContext) {
  const patch = await createHumanPatch(session, context)
  return patch === null || patch.trim().length > 0
}

/** Computes and applies the human patch when a rendered baseline already exists. */
export async function handleHumanPatch(session: SessionState, context: RuntimeContext) {
  const patch = await createHumanPatch(session, context)

  if (patch === null) {
    return createPatchFlowResult({
      status: "synced",
      acceptedPatchPath: null,
      rejectedPatchPath: null,
    })
  }

  if (!patch.trim()) {
    return createPatchFlowResult({
      status: "synced",
      acceptedPatchPath: null,
      rejectedPatchPath: null,
    })
  }

  const check = await git(session.agentWorktree, ["apply", "--check", "--binary"], context, {
    allowFailure: true,
    stdin: patch,
  })
  if (check.status !== 0) {
    const rejectedPatchPath = await savePatch(session, "rejected", patch)
    return createPatchFlowResult({
      status: "rejected-human-patch",
      acceptedPatchPath: null,
      rejectedPatchPath,
    })
  }

  const acceptedPatchPath = await savePatch(session, "accepted", patch)
  const apply = await git(session.agentWorktree, ["apply", "--binary"], context, {
    allowFailure: true,
    stdin: patch,
  })
  if (apply.status !== 0) {
    throw new UserError(
      `Human patch passed preflight but failed during apply; recovery is required in ${session.agentWorktree}.`,
    )
  }

  return createPatchFlowResult({
    status: "synced",
    acceptedPatchPath,
    rejectedPatchPath: null,
  })
}

/** Preserves the narrow patch-flow status type across async control-flow branches. */
function createPatchFlowResult(result: PatchFlowResult) {
  return result
}
