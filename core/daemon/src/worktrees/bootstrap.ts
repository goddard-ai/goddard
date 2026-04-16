/** Daemon-owned preparation helpers for fresh linked session worktrees. */
import type {
  WorktreeBootstrapConfig,
  WorktreeBootstrapPackageManager,
} from "@goddard-ai/schema/config"
import { constants as fsConstants } from "node:fs"
import { cp, mkdir, readFile, stat } from "node:fs/promises"
import * as path from "node:path"

import { runCommand } from "./process.ts"

const defaultSeedNames = ["node_modules", "dist", ".turbo"] as const
const supportedLockfiles = {
  bun: "bun.lock",
  pnpm: "pnpm-lock.yaml",
  npm: "package-lock.json",
  yarn: "yarn.lock",
} satisfies Record<WorktreeBootstrapPackageManager, string>

/**
 * Summary of one completed fresh-worktree preparation pass.
 */
export interface PreparedFreshWorktree {
  packageManager: WorktreeBootstrapPackageManager | null
  bootstrapRan: boolean
  seededPaths: string[]
}

type BootstrapEvent = {
  type: string
  detail: Record<string, unknown>
}

/**
 * Seeds selected untracked artifacts and runs any daemon-owned bootstrap command for one fresh worktree.
 */
export async function prepareFreshWorktree(input: {
  repoRoot: string
  worktreeDir: string
  config?: WorktreeBootstrapConfig
  onEvent?: (event: BootstrapEvent) => void | Promise<void>
}) {
  const config = input.config ?? {}
  const seededPaths: string[] = []
  let packageManager: WorktreeBootstrapPackageManager | null = null
  let bootstrapRan = false

  await emitEvent(input.onEvent, "worktree.bootstrap_started", {
    repoRoot: input.repoRoot,
    worktreeDir: input.worktreeDir,
  })

  if (config.enabled === false) {
    await emitEvent(input.onEvent, "worktree.bootstrap_skipped", {
      reason: "disabled",
    })

    return {
      packageManager,
      bootstrapRan,
      seededPaths,
    } satisfies PreparedFreshWorktree
  }

  if (config.seedEnabled !== false) {
    const sourceHead = await resolveHeadOid(input.repoRoot)
    const worktreeHead = await resolveHeadOid(input.worktreeDir)

    if (!sourceHead || !worktreeHead) {
      await emitEvent(input.onEvent, "worktree.seed_skipped", {
        reason: "missing_head",
      })
    } else if (sourceHead !== worktreeHead) {
      await emitEvent(input.onEvent, "worktree.seed_skipped", {
        reason: "head_mismatch",
        sourceHead,
        worktreeHead,
      })
    } else {
      const copiedPaths = await seedUntrackedPaths({
        repoRoot: input.repoRoot,
        worktreeDir: input.worktreeDir,
        seedNames: config.seedNames ?? [...defaultSeedNames],
        seedPaths: config.seedPaths ?? [],
        onEvent: input.onEvent,
      })
      seededPaths.push(...copiedPaths)
    }
  } else {
    await emitEvent(input.onEvent, "worktree.seed_skipped", {
      reason: "disabled",
    })
  }

  packageManager = await resolveBootstrapPackageManager(input.repoRoot, config.packageManager)
  if (!packageManager) {
    await emitEvent(input.onEvent, "worktree.bootstrap_skipped", {
      reason: "no_package_manager",
    })

    return {
      packageManager,
      bootstrapRan,
      seededPaths,
    } satisfies PreparedFreshWorktree
  }

  await emitEvent(input.onEvent, "worktree.bootstrap_selected_package_manager", {
    packageManager,
  })

  try {
    const result = await runCommand(packageManager, ["install", ...(config.installArgs ?? [])], {
      cwd: input.worktreeDir,
      stdin: "ignore",
    })

    if (result.status !== 0) {
      throw new Error(
        result.stderr.trim() || result.stdout.trim() || "install exited unsuccessfully",
      )
    }
  } catch (error) {
    throw new Error(
      `Fresh worktree bootstrap failed with ${packageManager}: ${error instanceof Error ? error.message : String(error)}`,
      {
        cause: error,
      },
    )
  }

  bootstrapRan = true
  await emitEvent(input.onEvent, "worktree.bootstrap_completed", {
    packageManager,
  })

  return {
    packageManager,
    bootstrapRan,
    seededPaths,
  } satisfies PreparedFreshWorktree
}

/**
 * Resolves one effective package manager from explicit config, package metadata, or lockfiles.
 */
async function resolveBootstrapPackageManager(
  repoRoot: string,
  configuredPackageManager?: WorktreeBootstrapPackageManager,
) {
  if (configuredPackageManager) {
    return configuredPackageManager
  }

  const packageJsonPackageManager = await resolvePackageManagerFromPackageJson(repoRoot)
  if (packageJsonPackageManager) {
    return packageJsonPackageManager
  }

  const detectedLockfiles = await Promise.all(
    Object.entries(supportedLockfiles).map(async ([manager, filename]) => {
      const lockfilePath = path.join(repoRoot, filename)
      return (await pathExists(lockfilePath)) ? (manager as WorktreeBootstrapPackageManager) : null
    }),
  )

  const recognizedManagers = detectedLockfiles.filter(
    (value): value is WorktreeBootstrapPackageManager => value !== null,
  )

  return recognizedManagers.length === 1 ? recognizedManagers[0] : null
}

/**
 * Reads one package manager hint from `package.json` when present and recognized.
 */
async function resolvePackageManagerFromPackageJson(repoRoot: string) {
  const packageJsonPath = path.join(repoRoot, "package.json")
  if (!(await pathExists(packageJsonPath))) {
    return null
  }

  try {
    const parsed = JSON.parse(await readFile(packageJsonPath, "utf-8")) as {
      packageManager?: unknown
    }
    if (typeof parsed.packageManager !== "string" || parsed.packageManager.length === 0) {
      return null
    }

    const packageManager = parsed.packageManager.split("@", 1)[0]
    return isSupportedPackageManager(packageManager) ? packageManager : null
  } catch {
    return null
  }
}

/**
 * Copies any configured untracked seed candidates into one fresh worktree.
 */
async function seedUntrackedPaths(input: {
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
 * Resolves one HEAD commit OID when the checkout currently has one.
 */
async function resolveHeadOid(cwd: string) {
  const result = await runCommand("git", ["rev-parse", "HEAD"], {
    cwd,
    stdin: "ignore",
  })

  if (result.status !== 0) {
    return null
  }

  const oid = result.stdout.trim()
  return oid.length > 0 ? oid : null
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
 * Returns true when one arbitrary value identifies a supported package manager.
 */
function isSupportedPackageManager(value: string): value is WorktreeBootstrapPackageManager {
  return value in supportedLockfiles
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
