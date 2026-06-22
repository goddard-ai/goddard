import { getSharedGitApi } from "./command"

/** Reads porcelain working tree status without mutating the repository. */
export async function getWorkingTreeStatus(rootDir: string) {
  return await getSharedGitApi().status.getWorkingTreeStatus(rootDir)
}
