import { runGit } from "../git/command"
import { branchExists } from "../git/refs"
import { getWorkingTreeStatus } from "../git/worktree"
import type { SprintBranchState, SprintDiagnostic } from "../types"
import type { SprintBranchWorktree, WorktreeEntry } from "./types"

/** Lists recorded sprint branches that cleanup can delete after landing. */
export async function cleanupBranches(rootDir: string, state: SprintBranchState) {
  const branches = [state.branches.review]
  if (await branchExists(rootDir, state.branches.approved)) {
    branches.push(state.branches.approved)
  }
  if (await branchExists(rootDir, state.branches.next)) {
    branches.push(state.branches.next)
  }
  return branches
}

/** Finds clean worktrees that must detach before cleanup deletes sprint branches. */
export async function sprintBranchWorktrees(
  rootDir: string,
  branchesToDelete: string[],
  diagnostics: SprintDiagnostic[],
) {
  const branchSet = new Set(branchesToDelete)
  const worktrees: SprintBranchWorktree[] = []

  for (const worktree of await listWorktrees(rootDir)) {
    const branchMatch = worktree.branch && branchSet.has(worktree.branch)
    if (!branchMatch) {
      continue
    }

    const status = await getWorkingTreeStatus(worktree.path)
    if (!status.clean) {
      diagnostics.push({
        severity: "error",
        code: "dirty_sprint_branch_worktree",
        message: `Sprint branch worktree ${worktree.path} is dirty.`,
      })
      continue
    }

    worktrees.push({
      ...worktree,
      reason: `branch ${worktree.branch}`,
    })
  }

  return worktrees
}

export async function listWorktrees(rootDir: string) {
  const output = await runGit(rootDir, ["worktree", "list", "--porcelain"])
  const entries: WorktreeEntry[] = []
  let current: WorktreeEntry | null = null

  for (const line of output.split("\n")) {
    if (line.length === 0) {
      if (current) {
        entries.push(current)
        current = null
      }
      continue
    }
    if (line.startsWith("worktree ")) {
      if (current) {
        entries.push(current)
      }
      current = {
        path: line.slice("worktree ".length),
        head: null,
        branch: null,
        detached: false,
      }
      continue
    }
    if (!current) {
      continue
    }
    if (line.startsWith("HEAD ")) {
      current.head = line.slice("HEAD ".length)
    } else if (line.startsWith("branch refs/heads/")) {
      current.branch = line.slice("branch refs/heads/".length)
    } else if (line === "detached") {
      current.detached = true
    }
  }

  if (current) {
    entries.push(current)
  }

  return entries
}
