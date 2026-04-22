/** Daemon helpers for reusing and cleaning up session-owned worktrees. */
import { realpathSync } from "node:fs"
import { resolve } from "node:path"
import type { DaemonWorktree } from "@goddard-ai/schema/daemon/store"
import type { WorktreePlugin } from "@goddard-ai/worktree-plugin"

import { deleteWorktree } from "../worktrees/index.ts"

const builtinWorktreePluginNames = new Set(["default", "worktrunk"])

/** Persisted worktree state stored separately from the base daemon session record. */
export type SessionWorktreeState = Omit<DaemonWorktree, "id" | "sessionId">

/** Prepared worktree state returned when one daemon session opts into isolation. */
export interface PreparedSessionWorktree {
  state: SessionWorktreeState
  logContext: Record<string, unknown>
}

/**
 * Reuses one persisted worktree by validating its plugin dependency and refreshing its branch metadata.
 */
export async function reuseExistingWorktree(
  worktree: SessionWorktreeState,
  params: {
    /**
     * Custom integration plugins to use for worktree reuse and later cleanup.
     */
    worktreePlugins?: WorktreePlugin[]
  } = {},
) {
  if (
    builtinWorktreePluginNames.has(worktree.poweredBy) === false &&
    params.worktreePlugins?.some((plugin) => plugin.name === worktree.poweredBy) !== true
  ) {
    throw new Error(
      `Missing worktree plugin "${worktree.poweredBy}" required to reuse ${worktree.worktreeDir}`,
    )
  }

  const headRef = await resolveExistingWorktreeHeadRef(worktree.worktreeDir)
  if (headRef) {
    worktree.branchName = headRef
  }
}

/**
 * Removes one daemon session worktree using the metadata recorded at creation time.
 */
export async function cleanupSessionWorktree(
  metadata: SessionWorktreeState,
  params: {
    /**
     * Custom integration plugins to use for worktree cleanup.
     */
    worktreePlugins?: WorktreePlugin[]
  } = {},
) {
  return await deleteWorktree({
    cwd: metadata.repoRoot,
    plugins: params.worktreePlugins,
    worktreeDir: metadata.worktreeDir,
    branchName: metadata.branchName,
    poweredBy: metadata.poweredBy,
  })
}

/**
 * Resolves the containing git repository root for one requested session cwd when one exists.
 */
export async function resolveGitRepoRoot(cwd: string) {
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
 * Converts persisted worktree metadata into the logging wrapper used by session launch.
 */
export function toPreparedSessionWorktree(state: SessionWorktreeState): PreparedSessionWorktree {
  return {
    state,
    logContext: {
      worktreeDir: state.worktreeDir,
      worktreePoweredBy: state.poweredBy,
    },
  }
}

/**
 * Resolves the currently attached branch for one existing worktree folder when HEAD is not detached.
 */
async function resolveExistingWorktreeHeadRef(cwd: string) {
  const resolvedCwd = resolve(realpathSync.native(cwd))
  const gitWorktreeCheck = await runGit(resolvedCwd, ["rev-parse", "--git-dir"])
  if (!gitWorktreeCheck.success) {
    throw new Error(`Existing worktree folder must be a git worktree: ${resolvedCwd}`)
  }

  const { success, stdout } = await runGit(resolvedCwd, [
    "symbolic-ref",
    "--quiet",
    "--short",
    "HEAD",
  ])
  if (!success) {
    return null
  }

  const headRef = stdout.trim()
  if (!headRef) {
    return null
  }

  return headRef
}

/**
 * Runs one git subprocess asynchronously using Bun's native subprocess API.
 */
async function runGit(cwd: string, args: string[]) {
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
