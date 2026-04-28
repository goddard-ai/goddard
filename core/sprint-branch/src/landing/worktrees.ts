import path from "node:path"

import { runGit } from "../git/command"
import { branchExists } from "../git/refs"
import { getWorkingTreeStatus } from "../git/worktree"
import type { SprintBranchState, SprintDiagnostic } from "../types"
import type { AssociatedWorktree, WorktreeEntry } from "./types"

/** Lists recorded sprint branches that cleanup can delete after landing. */
export async function cleanupBranches(rootDir: string, state: SprintBranchState) {
  const branches = [state.branches.review, state.branches.approved]
  if (await branchExists(rootDir, state.branches.next)) {
    branches.push(state.branches.next)
  }
  return branches
}

/** Finds clean worktrees associated with a landed sprint review snapshot. */
export async function associatedWorktrees(
  rootDir: string,
  state: SprintBranchState,
  reviewCommit: string | null,
  branchesToDelete: string[],
  diagnostics: SprintDiagnostic[],
) {
  const currentRoot = path.resolve(rootDir)
  const branchSet = new Set(branchesToDelete)
  const worktrees: AssociatedWorktree[] = []

  for (const worktree of await listWorktrees(rootDir)) {
    const resolvedPath = path.resolve(worktree.path)
    const branchMatch = worktree.branch && branchSet.has(worktree.branch)
    const detachedReview = Boolean(
      worktree.detached && reviewCommit && worktree.head === reviewCommit,
    )
    if (!branchMatch && !detachedReview) {
      continue
    }

    const reason = branchMatch ? `branch ${worktree.branch}` : "detached review snapshot"
    if (resolvedPath === currentRoot) {
      diagnostics.push({
        severity: "error",
        code: "current_worktree_would_be_removed",
        message: `Current worktree is associated with the sprint by ${reason}.`,
      })
      continue
    }

    const status = await getWorkingTreeStatus(worktree.path)
    if (!status.clean) {
      diagnostics.push({
        severity: "error",
        code: "dirty_associated_worktree",
        message: `Associated worktree ${worktree.path} is dirty.`,
      })
      continue
    }

    worktrees.push({
      ...worktree,
      reason,
    })
  }

  return worktrees
}

async function listWorktrees(rootDir: string) {
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
