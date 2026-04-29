/** Git and filesystem helpers used by review-sync internals. */
import { spawn } from "node:child_process"
import { constants as fsConstants } from "node:fs"
import { access, realpath } from "node:fs/promises"
import { basename, isAbsolute, join, relative, resolve } from "node:path"

import { UserError } from "./errors.ts"
import type { CommandResult, RuntimeContext, WorktreeInfo } from "./types.ts"

/** Runs one Git command and returns captured stdout/stderr. */
export async function git(
  cwd: string,
  args: string[],
  _context: RuntimeContext,
  options: {
    allowFailure?: boolean
    stdin?: string | "ignore"
    env?: Record<string, string | undefined>
  } = {},
) {
  const result = await runCommand("git", args, {
    cwd,
    stdin: options.stdin,
    env: {
      ...process.env,
      ...options.env,
    },
  })

  if (result.status !== 0 && options.allowFailure !== true) {
    throw new Error(
      `git ${args.join(" ")} failed in ${cwd}: ${
        result.stderr.trim() || result.stdout.trim() || "unknown Git error"
      }`,
    )
  }

  return result
}

/** Resolves the repository root for a path or raises a user-facing error. */
export async function resolveRequiredRepoRoot(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["rev-parse", "--show-toplevel"], context, {
    allowFailure: true,
  })
  if (result.status !== 0 || !result.stdout.trim()) {
    throw new UserError(`Not a Git worktree: ${cwd}`)
  }
  return await normalizePath(result.stdout.trim())
}

/** Resolves one worktree's Git common directory as an absolute path. */
export async function resolveRequiredGitCommonDir(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["rev-parse", "--git-common-dir"], context)
  const value = result.stdout.trim()
  return await normalizePath(isAbsolute(value) ? value : resolve(cwd, value))
}

/** Resolves one worktree's per-worktree Git metadata directory as an absolute path. */
export async function resolveRequiredGitDir(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["rev-parse", "--git-dir"], context)
  const value = result.stdout.trim()
  return await normalizePath(isAbsolute(value) ? value : resolve(cwd, value))
}

/** Returns the attached branch name, or null for detached HEAD. */
export async function resolveCurrentBranch(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"], context, {
    allowFailure: true,
  })
  return result.status === 0 ? result.stdout.trim() || null : null
}

/** Checks whether a local branch already exists. */
export async function branchExists(cwd: string, branch: string, context: RuntimeContext) {
  const result = await git(
    cwd,
    ["show-ref", "--verify", "--quiet", `refs/heads/${branch}`],
    context,
    {
      allowFailure: true,
    },
  )
  return result.status === 0
}

/** Checks whether a worktree has tracked, unstaged, staged, or untracked non-ignored changes. */
export async function isWorktreeClean(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["status", "--porcelain=v1", "--untracked-files=all"], context)
  return !result.stdout.trim()
}

/** Reads one ref and returns null when it does not exist. */
export async function resolveRef(cwd: string, refName: string, context: RuntimeContext) {
  const result = await git(cwd, ["rev-parse", "--verify", "-q", refName], context, {
    allowFailure: true,
  })
  return result.status === 0 ? result.stdout.trim() || null : null
}

/** Updates or creates one hidden ref. */
export async function updateRef(
  cwd: string,
  refName: string,
  oid: string,
  context: RuntimeContext,
) {
  await git(cwd, ["update-ref", refName, oid], context)
}

/** Ensures a review branch is not checked out outside the configured review worktree. */
export async function assertReviewBranchNotCheckedOutElsewhere(input: {
  cwd: string
  reviewBranch: string
  reviewWorktree: string
  context: RuntimeContext
}) {
  const worktrees = await listGitWorktrees(input.cwd, input.context)
  for (const worktree of worktrees) {
    if (worktree.branch !== input.reviewBranch) {
      continue
    }
    if (worktree.path !== input.reviewWorktree) {
      throw new UserError(
        `Review branch ${input.reviewBranch} is already checked out at ${worktree.path}.`,
      )
    }
  }
}

/** Refuses sync while Git has an unresolved operation in progress. */
export async function assertSupportedGitState(cwd: string, context: RuntimeContext) {
  const gitDir = await resolveRequiredGitDir(cwd, context)
  const commonDir = await resolveRequiredGitCommonDir(cwd, context)
  const markers = [
    join(gitDir, "MERGE_HEAD"),
    join(gitDir, "CHERRY_PICK_HEAD"),
    join(gitDir, "REVERT_HEAD"),
    join(gitDir, "REBASE_HEAD"),
    join(gitDir, "rebase-merge"),
    join(gitDir, "rebase-apply"),
    join(gitDir, "BISECT_LOG"),
    join(commonDir, "BISECT_LOG"),
  ]

  for (const marker of markers) {
    if (await pathExists(marker)) {
      throw new UserError(`Unsupported in-progress Git state in ${cwd}: ${basename(marker)}.`)
    }
  }
}

/** Resolves and canonicalizes an existing filesystem path. */
export async function normalizePath(path: string) {
  return await realpath(resolve(path))
}

/** Tests whether child is equal to or nested under parent. */
export function isInsideOrEqual(parent: string, child: string) {
  const rel = relative(parent, child)
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel))
}

/** Checks whether one filesystem path currently exists. */
export async function pathExists(path: string) {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

/** Checks whether a thrown value is a Node filesystem error with the requested code. */
export function isNodeErrorWithCode(error: unknown, code: string) {
  return (
    error instanceof Error &&
    "code" in error &&
    typeof error.code === "string" &&
    error.code === code
  )
}

/** Checks whether a local process id still exists. */
export function isProcessAlive(pid: number) {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

/** Parses Git's porcelain worktree list into path and branch records. */
export async function listGitWorktrees(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["worktree", "list", "--porcelain"], context)
  const worktrees: WorktreeInfo[] = []
  let current: Partial<WorktreeInfo> = {}

  for (const line of result.stdout.split("\n")) {
    if (!line.trim()) {
      if (current.path) {
        worktrees.push({
          path: await normalizePath(current.path),
          branch: current.branch ?? null,
        })
      }
      current = {}
      continue
    }

    if (line.startsWith("worktree ")) {
      current.path = line.slice("worktree ".length)
    } else if (line.startsWith("branch refs/heads/")) {
      current.branch = line.slice("branch refs/heads/".length)
    }
  }

  if (current.path) {
    worktrees.push({
      path: await normalizePath(current.path),
      branch: current.branch ?? null,
    })
  }

  return worktrees
}

/** Runs a subprocess with captured output and optional stdin. */
async function runCommand(
  command: string,
  args: string[],
  options: {
    cwd: string
    stdin?: string | "ignore"
    env: Record<string, string | undefined>
  },
) {
  return await new Promise<CommandResult>((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: ["pipe", "pipe", "pipe"],
    })

    let stdout = ""
    let stderr = ""
    child.stdout.setEncoding("utf8")
    child.stderr.setEncoding("utf8")
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk
    })
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk
    })
    child.on("error", rejectPromise)
    child.on("close", (status) => {
      resolvePromise({
        status: status ?? 1,
        stdout,
        stderr,
      })
    })

    if (options.stdin && options.stdin !== "ignore") {
      child.stdin.end(options.stdin)
    } else {
      child.stdin.end()
    }
  })
}
