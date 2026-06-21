import { getSharedGitHost } from "./command"

/** Reads porcelain working tree status without mutating the repository. */
export async function getWorkingTreeStatus(rootDir: string) {
  return await getSharedGitHost().status.getWorkingTreeStatus(rootDir)
}
