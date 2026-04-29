import * as fs from "node:fs/promises"
import path from "node:path"

import { GitCommandError, runGit } from "./command"

/** Resolves the root directory of the Git repository containing the start directory. */
export async function resolveRepositoryRoot(startDir: string) {
  return (await runGit(startDir, ["rev-parse", "--show-toplevel"])).trim()
}

/** Resolves a repository-local path inside Git's private metadata directory. */
export async function resolveGitPath(rootDir: string, gitPath: string) {
  const resolved = (await runGit(rootDir, ["rev-parse", "--git-path", gitPath])).trim()
  return path.isAbsolute(resolved) ? resolved : path.join(rootDir, resolved)
}

/** Resolves a path inside Git's common metadata directory shared by linked worktrees. */
export async function resolveGitCommonPath(rootDir: string, gitPath: string) {
  const resolved = (await runGit(rootDir, ["rev-parse", "--git-common-dir"])).trim()
  const commonDir = path.isAbsolute(resolved) ? resolved : path.join(rootDir, resolved)
  return path.join(commonDir, gitPath)
}

/** Resolves the current branch, returning null for detached HEAD. */
export async function getCurrentBranch(rootDir: string) {
  try {
    return (await runGit(rootDir, ["symbolic-ref", "--quiet", "--short", "HEAD"])).trim()
  } catch (error) {
    if (error instanceof GitCommandError) {
      return null
    }
    throw error
  }
}

/** Detects Git sequencer operations that need manual completion before retrying a command. */
export async function getGitOperations(rootDir: string) {
  const operationPaths = [
    ["rebase", "rebase-merge"],
    ["rebase", "rebase-apply"],
    ["merge", "MERGE_HEAD"],
    ["cherry-pick", "CHERRY_PICK_HEAD"],
    ["revert", "REVERT_HEAD"],
    ["bisect", "BISECT_LOG"],
  ]
  const operations: Array<{ name: string; path: string }> = []

  for (const [name, gitPath] of operationPaths) {
    const resolvedPath = await resolveGitPath(rootDir, gitPath)
    if (await pathExists(resolvedPath)) {
      operations.push({ name, path: resolvedPath })
    }
  }

  return operations
}

async function pathExists(targetPath: string) {
  try {
    await fs.access(targetPath)
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
}

function isMissingFileError(error: unknown) {
  return (
    error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === "ENOENT"
  )
}
