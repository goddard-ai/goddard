/** Git and filesystem helpers used by review-sync internals. */
import { constants as fsConstants } from "node:fs"
import { access } from "node:fs/promises"
import { basename, isAbsolute, join, relative, resolve } from "node:path"
import {
  createGitHost as createSharedGitHost,
  GitNotRepositoryError,
  normalizePath as normalizeSharedPath,
  resetGitHostForTests,
  type GitHost as SharedGitHost,
  type WorktreeInfo,
} from "@goddard-ai/libgit2"

import { UserError } from "./errors.ts"
import {
  runReviewSyncGitCommand,
  type GitCommandResult,
  type GitRunOptions,
} from "./git/command.ts"
import type { RuntimeContext } from "./types.ts"

export type { GitRunOptions, WorktreeInfo }

/** Captured subprocess result used by Git command wrappers. */
export type CommandResult = GitCommandResult

/** Internal Git access boundary used by review-sync operations. */
export type GitHost = {
  run: (cwd: string, args: string[], options?: GitRunOptions) => Promise<CommandResult>
  resolveRequiredRepoRoot: (cwd: string) => Promise<string>
  resolveRequiredGitCommonDir: (cwd: string) => Promise<string>
  resolveRequiredGitDir: (cwd: string) => Promise<string>
  resolveCurrentBranch: (cwd: string) => Promise<string | null>
  branchExists: (cwd: string, branch: string) => Promise<boolean>
  isWorktreeClean: (cwd: string) => Promise<boolean>
  resolveRef: (cwd: string, refName: string) => Promise<string | null>
  updateRef: (cwd: string, refName: string, oid: string) => Promise<void>
  deleteRef: (cwd: string, refName: string) => Promise<void>
  listWorktrees: (cwd: string) => Promise<WorktreeInfo[]>
}

/** Creates the Git host selected for this process. */
export function createReviewSyncGitHost(options: { libgit2PathCandidates?: string[] } = {}) {
  const commandHost = createReviewSyncCommandGitHost()
  const libgit2Host = adaptSharedGitHost(
    createSharedGitHost({
      libgit2PathCandidates: options.libgit2PathCandidates,
    }),
  )

  return {
    ...commandHost,
    resolveRequiredRepoRoot: libgit2Host.resolveRequiredRepoRoot,
    resolveRequiredGitCommonDir: libgit2Host.resolveRequiredGitCommonDir,
    resolveRequiredGitDir: libgit2Host.resolveRequiredGitDir,
    resolveCurrentBranch: libgit2Host.resolveCurrentBranch,
    branchExists: libgit2Host.branchExists,
    resolveRef: libgit2Host.resolveRef,
  }
}

export function resetReviewSyncGitHostForTests() {
  resetGitHostForTests()
}

function adaptSharedGitHost(shared: SharedGitHost): GitHost {
  return {
    run: runReviewSyncGitCommand,
    resolveRequiredRepoRoot: (cwd) => mapSharedGitErrors(() => shared.repository.resolveRoot(cwd)),
    resolveRequiredGitCommonDir: (cwd) =>
      mapSharedGitErrors(() => shared.repository.resolveCommonDir(cwd)),
    resolveRequiredGitDir: (cwd) => mapSharedGitErrors(() => shared.repository.resolveGitDir(cwd)),
    resolveCurrentBranch: (cwd) => shared.refs.getCurrentBranch(cwd),
    branchExists: (cwd, branch) => shared.refs.branchExists(cwd, branch),
    isWorktreeClean: (cwd) => shared.status.isWorktreeClean(cwd),
    resolveRef: (cwd, refName) => shared.refs.resolve(cwd, refName),
    updateRef: (cwd, refName, oid) => shared.refs.update(cwd, refName, oid),
    deleteRef: (cwd, refName) => shared.refs.delete(cwd, refName),
    listWorktrees: (cwd) => shared.worktrees.list(cwd),
  }
}

function createReviewSyncCommandGitHost(): GitHost {
  return {
    run: runReviewSyncGitCommand,
    resolveRequiredRepoRoot: async (cwd) => {
      const result = await runReviewSyncGitCommand(cwd, ["rev-parse", "--show-toplevel"], {
        allowFailure: true,
      })
      if (result.status !== 0 || !result.stdout.trim()) {
        throw new UserError(`Not a Git worktree: ${cwd}`)
      }
      return await normalizePath(result.stdout.trim())
    },
    resolveRequiredGitCommonDir: async (cwd) => {
      const result = await runReviewSyncGitCommand(cwd, ["rev-parse", "--git-common-dir"])
      return await normalizePath(resolveGitOutputPath(cwd, result.stdout.trim()))
    },
    resolveRequiredGitDir: async (cwd) => {
      const result = await runReviewSyncGitCommand(cwd, ["rev-parse", "--git-dir"])
      return await normalizePath(resolveGitOutputPath(cwd, result.stdout.trim()))
    },
    resolveCurrentBranch: async (cwd) => {
      const result = await runReviewSyncGitCommand(
        cwd,
        ["symbolic-ref", "--quiet", "--short", "HEAD"],
        {
          allowFailure: true,
        },
      )
      return result.status === 0 ? result.stdout.trim() || null : null
    },
    branchExists: async (cwd, branch) => {
      const result = await runReviewSyncGitCommand(
        cwd,
        ["show-ref", "--verify", "--quiet", `refs/heads/${branch}`],
        {
          allowFailure: true,
        },
      )
      return result.status === 0
    },
    isWorktreeClean: async (cwd) => {
      const result = await runReviewSyncGitCommand(cwd, [
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
      ])
      return (
        result.stdout
          .split("\n")
          .map((entry) => entry.trimEnd())
          .filter(Boolean).length === 0
      )
    },
    resolveRef: async (cwd, refName) => {
      const result = await runReviewSyncGitCommand(cwd, ["rev-parse", "--verify", "-q", refName], {
        allowFailure: true,
      })
      return result.status === 0 ? result.stdout.trim() || null : null
    },
    updateRef: async (cwd, refName, oid) => {
      await runReviewSyncGitCommand(cwd, ["update-ref", refName, oid])
    },
    deleteRef: async (cwd, refName) => {
      await runReviewSyncGitCommand(cwd, ["update-ref", "-d", refName], {
        allowFailure: true,
      })
    },
    listWorktrees: async (cwd) => {
      const result = await runReviewSyncGitCommand(cwd, ["worktree", "list", "--porcelain"])
      return parseGitWorktrees(result.stdout)
    },
  }
}

async function mapSharedGitErrors<T>(operation: () => Promise<T>) {
  try {
    return await operation()
  } catch (error) {
    if (error instanceof GitNotRepositoryError) {
      throw new UserError(error.message)
    }
    throw error
  }
}

/** Runs one Git command and returns captured stdout/stderr. */
export async function git(
  cwd: string,
  args: string[],
  context: RuntimeContext,
  options: GitRunOptions = {},
) {
  return await context.gitHost.run(cwd, args, options)
}

/** Resolves the repository root for a path or raises a user-facing error. */
export async function resolveRequiredRepoRoot(cwd: string, context: RuntimeContext) {
  return await context.gitHost.resolveRequiredRepoRoot(cwd)
}

/** Resolves one worktree's Git common directory as an absolute path. */
export async function resolveRequiredGitCommonDir(cwd: string, context: RuntimeContext) {
  return await context.gitHost.resolveRequiredGitCommonDir(cwd)
}

/** Resolves one worktree's per-worktree Git metadata directory as an absolute path. */
export async function resolveRequiredGitDir(cwd: string, context: RuntimeContext) {
  return await context.gitHost.resolveRequiredGitDir(cwd)
}

/** Returns the attached branch name, or null for detached HEAD. */
export async function resolveCurrentBranch(cwd: string, context: RuntimeContext) {
  return await context.gitHost.resolveCurrentBranch(cwd)
}

/** Checks whether a local branch already exists. */
export async function branchExists(cwd: string, branch: string, context: RuntimeContext) {
  return await context.gitHost.branchExists(cwd, branch)
}

/** Checks whether a worktree has tracked, unstaged, staged, or untracked non-ignored changes. */
export async function isWorktreeClean(cwd: string, context: RuntimeContext) {
  return await context.gitHost.isWorktreeClean(cwd)
}

/** Reads one ref and returns null when it does not exist. */
export async function resolveRef(cwd: string, refName: string, context: RuntimeContext) {
  return await context.gitHost.resolveRef(cwd, refName)
}

/** Updates or creates one hidden ref. */
export async function updateRef(
  cwd: string,
  refName: string,
  oid: string,
  context: RuntimeContext,
) {
  await context.gitHost.updateRef(cwd, refName, oid)
}

/** Deletes one hidden ref, allowing retry after a previous cleanup removed it. */
export async function deleteRef(cwd: string, refName: string, context: RuntimeContext) {
  await context.gitHost.deleteRef(cwd, refName)
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

  const rebaseHead = join(gitDir, "REBASE_HEAD")
  if ((await pathExists(rebaseHead)) && !(await isStaleRebaseHead(cwd, context))) {
    throw new UserError(`Unsupported in-progress Git state in ${cwd}: ${basename(rebaseHead)}.`)
  }
}

/** Distinguishes abandoned REBASE_HEAD files from a Git-recognized active rebase. */
async function isStaleRebaseHead(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["rebase", "--show-current-patch"], context, {
    allowFailure: true,
  })
  if (result.status === 0) {
    return false
  }

  return `${result.stderr}\n${result.stdout}`.toLowerCase().includes("no rebase in progress")
}

/** Resolves and canonicalizes an existing filesystem path. */
export async function normalizePath(path: string) {
  return await normalizeSharedPath(path)
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
  return await context.gitHost.listWorktrees(cwd)
}

function parseGitWorktrees(output: string): WorktreeInfo[] {
  const entries: WorktreeInfo[] = []
  let current: WorktreeInfo | null = null

  for (const line of output.split("\n")) {
    if (line.startsWith("worktree ")) {
      if (current) {
        entries.push(current)
      }
      current = { path: line.slice("worktree ".length), branch: null }
      continue
    }

    if (current && line.startsWith("branch refs/heads/")) {
      current.branch = line.slice("branch refs/heads/".length)
    }
  }

  if (current) {
    entries.push(current)
  }

  return entries
}

function resolveGitOutputPath(cwd: string, value: string) {
  return isAbsolute(value) ? value : resolve(cwd, value)
}
