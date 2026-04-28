import { runGit } from "../git/command"
import { getCurrentBranch } from "../git/repository"
import type { SprintBranchState } from "../types"

/** Moves a recorded sprint branch, using reset when that branch is checked out. */
export async function moveRecordedBranch(
  rootDir: string,
  state: SprintBranchState,
  branch: string,
  target: string,
) {
  if (!Object.values(state.branches).includes(branch)) {
    throw new Error(`Refusing to move unrecorded branch ${branch}.`)
  }

  if ((await getCurrentBranch(rootDir)) === branch) {
    await runGit(rootDir, ["reset", "--hard", target])
    return
  }

  await runGit(rootDir, ["branch", "--force", branch, target])
}

/** Describes the Git command that will move a branch for dry-run output. */
export function moveBranchOperation(branch: string, target: string, currentBranch: string | null) {
  return currentBranch === branch
    ? `git reset --hard ${target}`
    : `git branch --force ${branch} ${target}`
}
