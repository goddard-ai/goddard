import { getSharedGitApi } from "./command"

/** Checks whether a ref resolves to a commit. */
export async function refExists(rootDir: string, ref: string) {
  return await getSharedGitApi().refs.exists(rootDir, `${ref}^{commit}`)
}

/** Checks whether a local branch exists. */
export async function branchExists(rootDir: string, branch: string) {
  return await getSharedGitApi().refs.branchExists(rootDir, branch)
}

/** Returns the current commit for a local branch when it exists. */
export async function getBranchHead(rootDir: string, branch: string) {
  return await getSharedGitApi().refs.getBranchHead(rootDir, branch)
}

/** Checks whether the possible ancestor commit is contained by the descendant ref. */
export async function isAncestor(rootDir: string, ancestor: string, descendant: string) {
  return await getSharedGitApi().history.isAncestor(rootDir, ancestor, descendant)
}

/** Returns the best common ancestor for two refs when Git can identify one. */
export async function getMergeBase(rootDir: string, left: string, right: string) {
  return await getSharedGitApi().history.getMergeBase(rootDir, left, right)
}
