import type { SessionWorktreeMetadata } from "@goddard-ai/schema/daemon"
import { Worktree, type WorktreePlugin } from "@goddard-ai/worktree"
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
export async function prepareSessionWorktree(
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
): Promise<PreparedSessionWorktree | null> {
  const requestedCwd = resolve(realpathSync.native(cwd))
  const repoRoot = await resolveGitRepoRoot(requestedCwd)
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
        branchName: await resolveExistingWorktreeBranchName(existingFolder),
      }
    : await worktree.setup(params.branchNameOverride || `goddard-${sessionId}`)
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
export async function cleanupSessionWorktree(
  metadata: SessionWorktreeMetadata,
  params: {
    /**
     * Custom integration plugins to use for worktree cleanup.
     */
    worktreePlugins?: WorktreePlugin[]
  } = {},
): Promise<boolean> {
  const worktree = new Worktree({
    cwd: metadata.repoRoot,
    plugins: params.worktreePlugins,
  })
  return await worktree.cleanup(metadata.worktreeDir, metadata.branchName)
}

/**
 * Resolves the containing git repository root for one requested session cwd when one exists.
 */
async function resolveGitRepoRoot(cwd: string): Promise<string | null> {
  const { success, stdout } = await runGit(cwd, ["rev-parse", "--show-toplevel"])
  if (!success) {
    return null
  }

  const repoRoot = stdout.trim()
  if (!repoRoot) {
    return null
  }

  return resolve(repoRoot)
}

/**
 * Resolves the active branch name for one existing worktree folder.
 */
async function resolveExistingWorktreeBranchName(cwd: string): Promise<string> {
  const { success, stdout } = await runGit(cwd, ["rev-parse", "--abbrev-ref", "HEAD"])
  if (!success) {
    throw new Error(`Existing worktree folder must be a git worktree: ${cwd}`)
  }

  const branchName = stdout.trim()
  if (!branchName) {
    throw new Error(`Existing worktree folder must have a readable branch name: ${cwd}`)
  }

  return branchName
}

/**
 * Runs one git subprocess asynchronously using Bun's native subprocess API.
 */
async function runGit(
  cwd: string,
  args: string[],
): Promise<{ success: boolean; stdout: string }> {
  const result = Bun.spawn(["git", ...args], {
    cwd,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "ignore",
  })

  const stdout = result.stdout ? await new Response(result.stdout).text() : ""
  await result.exited
  return {
    success: result.exitCode === 0,
    stdout,
  }
}
