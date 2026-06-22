/** Session-owned preparation helpers for fresh linked session worktrees. */
import { readFile, stat } from "node:fs/promises"
import * as path from "node:path"
import { createGitHost } from "@goddard-ai/libgit2"

import type { WorktreeBootstrapConfig, WorktreeBootstrapPackageManager } from "../../schema.ts"
import { seedUntrackedPaths } from "./bootstrap/seed.ts"
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
 * Resolves one HEAD commit OID when the checkout currently has one.
 */
async function resolveHeadOid(cwd: string) {
  return await createGitHost().history.resolveHead(cwd)
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
