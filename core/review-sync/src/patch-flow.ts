/** Human patch acceptance and rejection flow. */
import { UserError } from "./errors.ts"
import { git, resolveRef } from "./git.ts"
import { createSnapshotCommit, diffCommits } from "./snapshot.ts"
import { savePatch } from "./state.ts"
import type { PatchFlowResult, RuntimeContext, SessionState } from "./types.ts"

/** Computes and applies the human patch when a rendered baseline already exists. */
export async function handleHumanPatch(session: SessionState, context: RuntimeContext) {
  const renderedSnapshot = await resolveRef(
    session.agentWorktree,
    session.refs.renderedSnapshot,
    context,
  )

  if (!renderedSnapshot) {
    return createPatchFlowResult({
      status: "synced",
      acceptedPatchPath: null,
      rejectedPatchPath: null,
    })
  }

  const reviewSnapshot = await createSnapshotCommit({
    cwd: session.reviewWorktree,
    label: `${session.sessionId}:review`,
    context,
  })
  const patch = await diffCommits(session.agentWorktree, renderedSnapshot, reviewSnapshot, context)

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
