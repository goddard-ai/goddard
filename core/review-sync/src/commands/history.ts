/** Shared Git history helpers for review-sync command modules. */
import { git } from "../git.ts"
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

/** Wraps Git's ancestry check with a clear failure for unexpected Git errors. */
export async function isAncestor(
  cwd: string,
  ancestor: string,
  descendant: string,
  context: RuntimeContext,
) {
  const result = await git(cwd, ["merge-base", "--is-ancestor", ancestor, descendant], context, {
    allowFailure: true,
  })
  if (result.status === 0) {
    return true
  }
  if (result.status === 1) {
    return false
  }
  throw new Error(
    `git merge-base --is-ancestor failed in ${cwd}: ${
      result.stderr.trim() || result.stdout.trim() || "unknown Git error"
    }`,
  )
}

/** Resolves the common baseline for divergent review and agent histories. */
export async function resolveMergeBase(
  cwd: string,
  left: string,
  right: string,
  context: RuntimeContext,
) {
  const result = await git(cwd, ["merge-base", left, right], context, {
    allowFailure: true,
  })
  if (result.status === 0) {
    return result.stdout.trim() || null
  }
  if (result.status === 1) {
    return null
  }
  throw new Error(
    `git merge-base failed in ${cwd}: ${
      result.stderr.trim() || result.stdout.trim() || "unknown Git error"
    }`,
  )
}
