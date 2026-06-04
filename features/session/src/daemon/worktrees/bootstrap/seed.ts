/** Seed candidate resolution and copying for fresh linked session worktrees. */
import { constants as fsConstants } from "node:fs"
import { cp, mkdir, stat } from "node:fs/promises"
import * as path from "node:path"

import { runCommand } from "../process.ts"

const worktreeIncludeFileName = ".worktreeinclude"

type BootstrapEvent = {
  type: string
  detail: Record<string, unknown>
}

/**
 * Copies any configured untracked seed candidates into one fresh worktree.
 */
export async function seedUntrackedPaths(input: {
  repoRoot: string
  worktreeDir: string
  seedNames: string[]
  seedPaths: string[]
  onEvent?: (event: BootstrapEvent) => void | Promise<void>
}) {
  const seedNames = new Set(input.seedNames)
  const untrackedEntries = await listUntrackedEntries(input.repoRoot)
  const candidates = new Set<string>()

  for (const entry of untrackedEntries) {
    const basename = path.basename(entry.relativePath)
    if (seedNames.has(basename)) {
      candidates.add(entry.relativePath)
    }
  }

  for (const configuredPath of input.seedPaths) {
    const normalizedPath = normalizeSeedPath(input.repoRoot, configuredPath)
    if (!normalizedPath) {
      await emitEvent(input.onEvent, "worktree.seed_skipped", {
        path: configuredPath,
        reason: "invalid_path",
      })
      continue
    }

    if (!(await pathExists(path.join(input.repoRoot, normalizedPath)))) {
      await emitEvent(input.onEvent, "worktree.seed_skipped", {
        path: normalizedPath,
        reason: "missing_source",
      })
      continue
    }

    if (!isCoveredByUntrackedEntries(normalizedPath, untrackedEntries)) {
      await emitEvent(input.onEvent, "worktree.seed_skipped", {
        path: normalizedPath,
        reason: "not_untracked",
      })
      continue
    }

    candidates.add(normalizedPath)
  }

  for (const relativePath of await listWorktreeIncludeCandidates(input.repoRoot)) {
    candidates.add(relativePath)
  }

  const copiedPaths: string[] = []
  for (const relativePath of [...candidates].sort()) {
    const sourcePath = path.join(input.repoRoot, relativePath)
    if (pathsOverlap(sourcePath, input.worktreeDir)) {
      await emitEvent(input.onEvent, "worktree.seed_skipped", {
        path: relativePath,
        reason: "overlaps_worktree",
      })
      continue
    }

    const targetPath = path.join(input.worktreeDir, relativePath)

    try {
      const copyMode = await copySeedCandidate(sourcePath, targetPath)
      copiedPaths.push(relativePath)
      await emitEvent(input.onEvent, "worktree.seed_copied", {
        path: relativePath,
        copyMode,
      })
    } catch (error) {
      if (isExistingPathError(error)) {
        await emitEvent(input.onEvent, "worktree.seed_skipped", {
          path: relativePath,
          reason: "target_exists",
        })
        continue
      }

      await emitEvent(input.onEvent, "worktree.seed_failed", {
        path: relativePath,
        errorMessage: error instanceof Error ? error.message : String(error),
      })
    }
  }

  if (copiedPaths.length === 0) {
    await emitEvent(input.onEvent, "worktree.seed_skipped", {
      reason: "no_candidates",
    })
  }

  return copiedPaths
}

/**
 * Lists ignored, untracked paths matched by the repository's `.worktreeinclude` file.
 */
async function listWorktreeIncludeCandidates(repoRoot: string) {
  const includePath = path.join(repoRoot, worktreeIncludeFileName)
  if (!(await pathExists(includePath))) {
    return []
  }

  const matchedByInclude = await listUntrackedEntriesMatchedByExcludeFile(
    repoRoot,
    worktreeIncludeFileName,
  )
  const ignoredPaths = await filterGitignoredPaths(
    repoRoot,
    matchedByInclude.map((entry) => entry.relativePath),
  )

  return matchedByInclude
    .filter((entry) => ignoredPaths.has(entry.relativePath))
    .map((entry) => entry.relativePath)
}

/**
 * Lists the repository-relative untracked entries Git currently exposes for one checkout.
 */
async function listUntrackedEntries(repoRoot: string) {
  const result = await runCommand(
    "git",
    ["ls-files", "--others", "--exclude-standard", "--directory"],
    {
      cwd: repoRoot,
      stdin: "ignore",
    },
  )

  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || "git ls-files failed")
  }

  return result.stdout
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const isDir = line.endsWith("/")
      return {
        relativePath: isDir ? line.slice(0, -1) : line,
        isDir,
      }
    })
}

/**
 * Lists untracked entries matched by one Git exclude-pattern file.
 */
async function listUntrackedEntriesMatchedByExcludeFile(repoRoot: string, excludeFile: string) {
  const result = await runCommand(
    "git",
    ["ls-files", "--others", "--ignored", "--directory", "-z", `--exclude-from=${excludeFile}`],
    {
      cwd: repoRoot,
      stdin: "ignore",
    },
  )

  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || "git ls-files failed")
  }

  return parseGitPathOutput(result.stdout).map((relativePath) => ({ relativePath }))
}

/**
 * Filters repository-relative paths down to those ignored by the repository's standard Git rules.
 */
async function filterGitignoredPaths(repoRoot: string, relativePaths: string[]) {
  if (relativePaths.length === 0) {
    return new Set<string>()
  }

  const normalizedPaths = relativePaths.map((relativePath) =>
    trimTrailingPathSeparator(relativePath),
  )
  const result = await runCommand("git", ["check-ignore", "--stdin", "-z"], {
    cwd: repoRoot,
    stdin: `${normalizedPaths.join("\0")}\0`,
  })

  if (result.status !== 0 && result.status !== 1) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || "git check-ignore failed")
  }

  return new Set(parseGitPathOutput(result.stdout).map(trimTrailingPathSeparator))
}

/**
 * Copies one seed candidate, preferring copy-on-write before falling back to a normal copy.
 */
async function copySeedCandidate(sourcePath: string, targetPath: string) {
  const sourceStats = await stat(sourcePath)
  const sharedOptions = {
    recursive: sourceStats.isDirectory(),
    force: false,
    errorOnExist: true,
  } as const

  await mkdir(path.dirname(targetPath), { recursive: true })

  try {
    await cp(sourcePath, targetPath, {
      ...sharedOptions,
      mode: reflinkModeForPlatform(),
    })
    return "copy_on_write"
  } catch (error) {
    if (isExistingPathError(error)) {
      throw error
    }

    await cp(sourcePath, targetPath, sharedOptions)
    return "copy"
  }
}

/**
 * Normalizes one configured repo-relative seed path when it stays inside the repository.
 */
function normalizeSeedPath(repoRoot: string, configuredPath: string) {
  const resolvedPath = path.resolve(repoRoot, configuredPath)
  const relativePath = path.relative(repoRoot, resolvedPath)

  if (relativePath.length === 0 || relativePath.startsWith("..") || path.isAbsolute(relativePath)) {
    return null
  }

  return relativePath
}

/**
 * Returns true when one repo-relative path is covered by the current Git untracked listing.
 */
function isCoveredByUntrackedEntries(
  relativePath: string,
  entries: Array<{ relativePath: string; isDir: boolean }>,
) {
  return entries.some((entry) => {
    if (entry.relativePath === relativePath) {
      return true
    }

    return entry.isDir && relativePath.startsWith(`${entry.relativePath}${path.sep}`)
  })
}

/**
 * Parses Git path output that may be NUL-delimited.
 */
function parseGitPathOutput(stdout: string) {
  return stdout
    .split("\0")
    .filter((line) => line.length > 0)
    .map(trimTrailingPathSeparator)
}

/**
 * Normalizes Git directory output to the repo-relative path used by copy targets.
 */
function trimTrailingPathSeparator(relativePath: string) {
  return relativePath.endsWith("/") ? relativePath.slice(0, -1) : relativePath
}

/**
 * Returns true when two filesystem paths overlap by ancestry or identity.
 */
function pathsOverlap(firstPath: string, secondPath: string) {
  return isWithinDir(firstPath, secondPath) || isWithinDir(secondPath, firstPath)
}

/**
 * Returns true when one filesystem path resolves inside another directory.
 */
function isWithinDir(parentDir: string, childPath: string) {
  const relativePath = path.relative(parentDir, childPath)
  return (
    relativePath.length === 0 || (!relativePath.startsWith("..") && !path.isAbsolute(relativePath))
  )
}

/**
 * Returns the copy mode flag used to request reflink copies on supported platforms.
 */
function reflinkModeForPlatform() {
  if (process.platform === "darwin" || process.platform === "linux") {
    return fsConstants.COPYFILE_FICLONE
  }

  return 0
}

/**
 * Returns true when one filesystem path exists.
 */
async function pathExists(targetPath: string) {
  try {
    await stat(targetPath)
    return true
  } catch {
    return false
  }
}

/**
 * Returns true when one filesystem-copy error indicates pre-existing target content.
 */
function isExistingPathError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "EEXIST"
  )
}

/**
 * Emits one optional bootstrap event without coupling the helper to logging or diagnostics storage.
 */
async function emitEvent(
  onEvent: ((event: BootstrapEvent) => void | Promise<void>) | undefined,
  type: string,
  detail: Record<string, unknown>,
) {
  await onEvent?.({
    type,
    detail,
  })
}
