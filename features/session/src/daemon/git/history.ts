import { git } from "@goddard-ai/libgit2"

export async function countCommitsAhead(cwd: string, baseOid: string) {
  return await git.history.countCommits(cwd, { from: baseOid })
}
