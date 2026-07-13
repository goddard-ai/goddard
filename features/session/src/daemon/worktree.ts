/** Daemon helpers for reusing and cleaning up session-owned worktrees. */
import { realpathSync } from "node:fs"
import { resolve } from "node:path"
import { git, GitNotRepositoryError } from "@goddard-ai/libgit2"
import type { WorktreePlugin } from "@goddard-ai/worktree-plugin"

import type { DaemonWorktree } from "../schema.ts"
import { countCommitsAhead } from "./git/history.ts"
import { deleteWorktree } from "./worktrees/index.ts"

const builtinWorktreePluginNames = new Set(["default", "worktrunk"])

/** Persisted worktree state stored separately from the base daemon session record. */
export type SessionWorktreeState = Omit<DaemonWorktree, "id" | "sessionId">

/** Prepared worktree state returned when one daemon session opts into isolation. */
export interface PreparedSessionWorktree {
  state: SessionWorktreeState
  logContext: Record<string, unknown>
}

/** User-facing completion blockers detected from one daemon-managed worktree. */
export type SessionWorktreeCompletionState = {
  dirty: boolean
  unmergedCommits: boolean
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

  const headRef = await resolveGitHeadRef(worktree.worktreeDir)
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
  try {
    return resolve(await git.repository.resolveRoot(cwd))
  } catch {
    return null
  }
}

/** Returns true when the requested cwd points at a bare git repository. */
export async function inspectGitBareRepository(cwd: string) {
  try {
    return await git.repository.isBareRepository(cwd)
  } catch (error) {
    if (error instanceof GitNotRepositoryError) {
      return false
    }
    throw error
  }
}

/** Resolves the git source directory that can create linked worktrees for one launch cwd. */
export async function resolveGitWorktreeSource(cwd: string) {
  const repoRoot = await resolveGitRepoRoot(cwd)
  if (repoRoot) {
    return {
      path: repoRoot,
      bare: false,
    }
  }

  if (!(await inspectGitBareRepository(cwd))) {
    return null
  }

  return {
    path: resolve(realpathSync.native(cwd)),
    bare: true,
  }
}

/**
 * Resolves the currently attached branch for one existing worktree folder when HEAD is not detached.
 */
export async function resolveGitHeadRef(cwd: string) {
  const resolvedCwd = resolve(realpathSync.native(cwd))
  try {
    await git.repository.resolveGitDir(resolvedCwd)
  } catch {
    throw new Error(`Existing worktree folder must be a git worktree: ${resolvedCwd}`)
  }

  return await git.refs.getCurrentBranch(resolvedCwd)
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
 * Inspects whether one daemon-managed worktree has user work that should block completion.
 */
export async function inspectWorktreeCompletionState(
  worktree: SessionWorktreeState,
): Promise<SessionWorktreeCompletionState> {
  const [status, primaryHead] = await Promise.all([
    git.status.getWorkingTreeStatus(worktree.worktreeDir),
    git.history.resolveHead(worktree.repoRoot),
  ])
  if (!primaryHead) {
    throw new Error("Unable to inspect primary checkout HEAD")
  }

  return {
    dirty: status.entries.length > 0,
    unmergedCommits: (await countCommitsAhead(worktree.worktreeDir, primaryHead)) > 0,
  }
}
