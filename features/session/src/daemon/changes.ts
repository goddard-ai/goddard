import { buildInitialWorkspaceDiff, buildTrackedAndUntrackedDiff, hasGitHead } from "./git/diff.ts"
import { resolveGitRepoRoot, type SessionWorktreeState } from "./worktree.ts"

/**
 * Reads the current git diff snapshot for one daemon session workspace.
 */
export async function readSessionChanges(params: {
  cwd: string
  worktree: SessionWorktreeState | null
}) {
  const workspaceRoot = params.worktree?.worktreeDir ?? (await resolveGitRepoRoot(params.cwd))

  if (!workspaceRoot) {
    return {
      workspaceRoot: null,
      diff: "",
      hasChanges: false,
    }
  }

  const diff = (await hasGitHead(workspaceRoot))
    ? await buildTrackedAndUntrackedDiff(workspaceRoot)
    : await buildInitialWorkspaceDiff(workspaceRoot)

  return {
    workspaceRoot,
    diff,
    hasChanges: diff.length > 0,
  }
}
