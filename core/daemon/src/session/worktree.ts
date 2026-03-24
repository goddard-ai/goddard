import type { SessionWorktreeMetadata } from "@goddard-ai/schema/daemon"
import { Worktree, type WorktreePlugin } from "@goddard-ai/worktree"
import { spawnSync } from "node:child_process"
import { realpathSync } from "node:fs"
import { join, relative, resolve } from "node:path"

/** Prepared worktree state returned when one daemon session opts into isolation. */
export interface PreparedSessionWorktree {
  worktree: Worktree
  effectiveCwd: string
  metadata: SessionWorktreeMetadata
  logContext: Record<string, unknown>
}

/**
 * Creates one daemon-owned worktree and maps the requested cwd into the cloned workspace.
 */
export function prepareSessionWorktree(
  sessionId: string,
  cwd: string,
  params: {
    /**
     * If provided, use this branch name instead of the default one.
     */
    branchNameOverride?: string
    /**
     * Custom integration plugins to use for worktree setup.
     */
    worktreePlugins?: WorktreePlugin[]
    /**
     * Use this already-provisioned folder instead of creating a new worktree.
     */
    existingFolder?: string
    /**
     * If the default worktree implementation is used, ensure this folder
     * exists in the repository root. Worktrees are stored in this folder.
     */
    defaultWorktreesFolder?: string
  } = {},
): PreparedSessionWorktree | null {
  const requestedCwd = resolve(realpathSync.native(cwd))
  const repoRoot = resolveGitRepoRoot(requestedCwd)
  if (!repoRoot) {
    return null
  }

  const relativeCwd = relative(repoRoot, requestedCwd)
  const worktree = new Worktree({
    cwd: repoRoot,
    plugins: params.worktreePlugins,
    defaultPluginDirName: params.defaultWorktreesFolder,
  })
  const existingFolder = params.existingFolder
    ? resolve(realpathSync.native(params.existingFolder))
    : null
  const { worktreeDir, branchName } = existingFolder
    ? {
        worktreeDir: existingFolder,
        branchName: resolveExistingWorktreeBranchName(existingFolder),
      }
    : worktree.setup(params.branchNameOverride || `goddard-${sessionId}`)
  const effectiveCwd = join(worktreeDir, relativeCwd)

  return {
    worktree,
    effectiveCwd,
    metadata: {
      repoRoot,
      requestedCwd,
      effectiveCwd,
      worktreeDir,
      branchName,
      poweredBy: worktree.poweredBy,
    },
    logContext: {
      worktreeDir,
      worktreePoweredBy: worktree.poweredBy,
    },
  }
}

/**
 * Removes one daemon session worktree using the metadata recorded at creation time.
 */
export function cleanupSessionWorktree(
  metadata: SessionWorktreeMetadata,
  params: {
    /**
     * Custom integration plugins to use for worktree cleanup.
     */
    worktreePlugins?: WorktreePlugin[]
  } = {},
): boolean {
  const worktree = new Worktree({
    cwd: metadata.repoRoot,
    plugins: params.worktreePlugins,
  })
  return worktree.cleanup(metadata.worktreeDir, metadata.branchName)
}

/**
 * Resolves the containing git repository root for one requested session cwd when one exists.
 */
function resolveGitRepoRoot(cwd: string): string | null {
  const { status, stdout } = spawnSync("git", ["rev-parse", "--show-toplevel"], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  })

  if (status !== 0) return null

  const repoRoot = stdout.trim()
  return repoRoot || null
}

/**
 * Resolves the active branch name for one existing worktree folder.
 */
function resolveExistingWorktreeBranchName(cwd: string): string {
  const { status, stdout } = spawnSync("git", ["rev-parse", "--abbrev-ref", "HEAD"], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  })

  if (status !== 0) {
    throw new Error(`Existing worktree folder must be a git worktree: ${cwd}`)
  }

  const branchName = stdout.trim()
  if (!branchName) {
    throw new Error(`Existing worktree folder must have a readable branch name: ${cwd}`)
  }

  return branchName
}
