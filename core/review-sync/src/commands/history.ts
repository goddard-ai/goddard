/** Shared Git history helpers for review-sync command modules. */
import type { RuntimeContext } from "../types.ts"

/** Checks whether the review branch contains commits not reachable from the agent ref. */
export async function reviewBranchHasHumanCommits(input: {
  cwd: string
  branchHead: string
  currentHead: string
  context: RuntimeContext
}) {
  if (await isAncestor(input.cwd, input.branchHead, input.currentHead, input.context)) {
    return true
  }
  if (await isAncestor(input.cwd, input.currentHead, input.branchHead, input.context)) {
    return false
  }
  return true
}

/** Checks whether one commit is reachable from another. */
export async function isAncestor(
  cwd: string,
  ancestor: string,
  descendant: string,
  context: RuntimeContext,
) {
  return await context.gitHost.isAncestor(cwd, ancestor, descendant)
}

/** Resolves the common baseline for divergent review and agent histories. */
export async function resolveMergeBase(
  cwd: string,
  left: string,
  right: string,
  context: RuntimeContext,
) {
  return await context.gitHost.getMergeBase(cwd, left, right)
}
