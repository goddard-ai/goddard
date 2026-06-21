import * as fs from "node:fs/promises"
import path from "node:path"

import { getSharedGitHost } from "./command"

/** Resolves the root directory of the Git repository containing the start directory. */
export async function resolveRepositoryRoot(startDir: string) {
  return await getSharedGitHost().repository.resolveRoot(startDir)
}

/** Resolves a repository-local path inside Git's private metadata directory. */
export async function resolveGitPath(rootDir: string, gitPath: string) {
  return getSharedGitHost().repository.resolveGitPath(rootDir, gitPath)
}

/** Ensures a Git-private exclude pattern exists without duplicating it. */
export async function ensureGitInfoExcludeEntry(rootDir: string, entry: string) {
  const excludePath = await resolveGitPath(rootDir, "info/exclude")
  await fs.mkdir(path.dirname(excludePath), { recursive: true })

  let existing = ""
  if (await pathExists(excludePath)) {
    existing = await fs.readFile(excludePath, "utf-8")
  }

  if (existing.split(/\r?\n/).some((line) => line.trim() === entry)) {
    return
  }

  const prefix = existing.length === 0 || existing.endsWith("\n") ? existing : `${existing}\n`
  await fs.writeFile(excludePath, `${prefix}${entry}\n`)
}

/** Resolves a path inside Git's common metadata directory shared by linked worktrees. */
export async function resolveGitCommonPath(rootDir: string, gitPath: string) {
  const commonDir = await getSharedGitHost().repository.resolveCommonDir(rootDir)
  return path.join(commonDir, gitPath)
}

/** Resolves the current branch, returning null for detached HEAD. */
export async function getCurrentBranch(rootDir: string) {
  return await getSharedGitHost().refs.getCurrentBranch(rootDir)
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
