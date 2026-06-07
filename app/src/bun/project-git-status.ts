import type { ProjectGitCheckoutSummary, ProjectGitStatus } from "~/shared/desktop-rpc.ts"

type GitResult = {
  stdout: string
  stderr: string
  success: boolean
}

async function runGit(cwd: string, args: readonly string[]): Promise<GitResult> {
  try {
    const childProcess = Bun.spawn(["git", ...args], {
      cwd,
      stderr: "pipe",
      stdout: "pipe",
    })
    const [stdout, stderr, exitCode] = await Promise.all([
      new Response(childProcess.stdout).text(),
      new Response(childProcess.stderr).text(),
      childProcess.exited,
    ])

    return {
      stdout,
      stderr,
      success: exitCode === 0,
    }
  } catch (error) {
    return {
      stdout: "",
      stderr: error instanceof Error ? error.message : "Unable to run git.",
      success: false,
    }
  }
}

function readBranchStatus(line: string) {
  const ahead = Number(line.match(/\[ahead (\d+)/)?.[1] ?? 0)
  const behind = Number(line.match(/\[.*behind (\d+)/)?.[1] ?? 0)
  const branch =
    line
      .replace(/^##\s*/, "")
      .split("...")[0]
      ?.trim() || null

  return {
    ahead,
    behind,
    branch: branch === "HEAD (no branch)" ? null : branch,
  }
}

async function readCheckoutSummary(path: string): Promise<ProjectGitCheckoutSummary> {
  const [headResult, statusResult] = await Promise.all([
    runGit(path, ["rev-parse", "--short", "HEAD"]),
    runGit(path, ["status", "--porcelain=v1", "-b"]),
  ])

  if (!statusResult.success) {
    return {
      path,
      branch: null,
      head: null,
      hasChanges: false,
      changedCount: 0,
      untrackedCount: 0,
      ahead: 0,
      behind: 0,
      errorMessage: statusResult.stderr.trim() || "This path is not a readable git checkout.",
    }
  }

  const lines = statusResult.stdout.split(/\r?\n/).filter(Boolean)
  const branchStatus = readBranchStatus(lines[0] ?? "")
  const entries = lines.slice(1)
  const untrackedCount = entries.filter((line) => line.startsWith("??")).length
  const changedCount = entries.length - untrackedCount

  return {
    path,
    branch: branchStatus.branch,
    head: headResult.success ? headResult.stdout.trim() || null : null,
    hasChanges: entries.length > 0,
    changedCount,
    untrackedCount,
    ahead: branchStatus.ahead,
    behind: branchStatus.behind,
    errorMessage: null,
  }
}

function parseWorktreePaths(output: string, primaryPath: string) {
  return output
    .split(/\r?\n/)
    .filter((line) => line.startsWith("worktree "))
    .map((line) => line.slice("worktree ".length).trim())
    .filter((path) => path.length > 0 && path !== primaryPath)
}

/** Reads git status for one primary checkout and any linked git worktrees. */
export async function getProjectGitStatus(path: string): Promise<ProjectGitStatus> {
  const primary = await readCheckoutSummary(path)

  if (primary.errorMessage) {
    return {
      primary,
      worktrees: [],
    }
  }

  const [primaryRootResult, worktreeResult] = await Promise.all([
    runGit(path, ["rev-parse", "--show-toplevel"]),
    runGit(path, ["worktree", "list", "--porcelain"]),
  ])
  const primaryPath = primaryRootResult.success ? primaryRootResult.stdout.trim() || path : path
  const worktreePaths = worktreeResult.success
    ? parseWorktreePaths(worktreeResult.stdout, primaryPath)
    : []

  return {
    primary,
    worktrees: await Promise.all(worktreePaths.map(readCheckoutSummary)),
  }
}
