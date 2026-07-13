import { git, GitNotRepositoryError } from "@goddard-ai/libgit2"

import { runGitCommand } from "./command.ts"

export async function addDetachedWorktree(cwd: string, worktreeDir: string) {
  const result = await runGitCommand(cwd, ["worktree", "add", "--detach", worktreeDir], {
    stdin: "ignore",
  })

  if (result.status !== 0) {
    throw new Error(
      `Failed to create linked worktree at ${worktreeDir}: ${result.stderr.trim() || result.stdout.trim() || "git worktree add exited unsuccessfully"}`,
    )
  }
}

export async function fetchPullRequestHead(params: {
  worktreeDir: string
  prNumber: string
  branchName: string
}) {
  await runGitCommand(
    params.worktreeDir,
    ["fetch", "origin", `pull/${params.prNumber}/head:${params.branchName}`],
    { stdin: "ignore" },
  )
}

export async function checkoutBranchFromFetchHead(worktreeDir: string, branchName: string) {
  await runGitCommand(worktreeDir, ["checkout", "-B", branchName, "FETCH_HEAD"], {
    stdin: "ignore",
  })
}

export async function checkoutWorktreeBranch(params: {
  worktreeDir: string
  branchName: string
  baseBranchName?: string
}) {
  await runGitCommand(
    params.worktreeDir,
    params.baseBranchName
      ? ["checkout", "-B", params.branchName, params.baseBranchName]
      : ["checkout", "-B", params.branchName],
    { stdin: "ignore" },
  )
}

export async function checkoutExistingBranch(worktreeDir: string, branchName: string) {
  await runGitCommand(worktreeDir, ["checkout", branchName], {
    stdin: "ignore",
  })
}

export async function removeWorktree(cwd: string, worktreeDir: string) {
  const result = await runGitCommand(cwd, ["worktree", "remove", "--force", worktreeDir], {
    stdin: "ignore",
  })

  return result.status === 0
}

export async function findWorktrunkBranchWorktree(cwd: string, branchName: string) {
  try {
    return (
      (await git.worktrees.list(cwd)).find((worktree) => worktree.branch === branchName)?.path ??
      null
    )
  } catch (error) {
    if (error instanceof GitNotRepositoryError) {
      return null
    }
    throw error
  }
}
