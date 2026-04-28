import { GitCommandError, gitSucceeds, runGit } from "./command"

/** Checks whether a local branch exists. */
export async function branchExists(rootDir: string, branch: string) {
  return gitSucceeds(rootDir, ["rev-parse", "--verify", "--quiet", `refs/heads/${branch}`])
}

/** Returns the current commit for a local branch when it exists. */
export async function getBranchHead(rootDir: string, branch: string) {
  try {
    return (await runGit(rootDir, ["rev-parse", "--verify", `refs/heads/${branch}`])).trim()
  } catch (error) {
    if (error instanceof GitCommandError) {
      return null
    }
    throw error
  }
}

/** Checks whether the possible ancestor commit is contained by the descendant ref. */
export async function isAncestor(rootDir: string, ancestor: string, descendant: string) {
  return gitSucceeds(rootDir, ["merge-base", "--is-ancestor", ancestor, descendant])
}
