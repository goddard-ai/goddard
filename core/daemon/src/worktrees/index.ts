/** Daemon-owned worktree creation and cleanup helpers with pluggable strategies. */
import type { WorktreePlugin, WorktreeSetupOptions } from "@goddard-ai/worktree-plugin"
import type { KindInput } from "kindstore"
import * as fs from "node:fs"
import * as path from "node:path"
import { defaultPlugin } from "./plugins/default.ts"
import { worktrunkPlugin } from "./plugins/worktrunk.ts"

export type { WorktreePlugin, WorktreeSetupOptions }

/**
 * Shared options for resolving or operating on one repository worktree.
 */
export interface WorktreeOptions {
  /**
   * Optional list of plugins to use for worktree management.
   * If provided, these will be evaluated before the built-in plugins.
   */
  plugins?: WorktreePlugin[]

  /**
   * The current working directory of the original repository.
   */
  cwd: string

  /**
   * The default directory name to use for created worktrees.
   */
  defaultPluginDirName?: string
}

/**
 * Options for creating one worktree for a target branch.
 */
export interface CreateWorktreeOptions extends WorktreeOptions {
  /**
   * The branch name to create or reuse inside the new worktree.
   */
  branchName: string

  /**
   * The requested working directory that should be mapped inside the created worktree.
   */
  requestedCwd?: string
}

/**
 * Options for deleting one previously created worktree.
 */
export interface DeleteWorktreeOptions extends WorktreeOptions {
  /**
   * The worktree directory to remove.
   */
  worktreeDir: string

  /**
   * The branch associated with the worktree.
   */
  branchName: string

  /**
   * The plugin name that originally created the worktree when known.
   */
  poweredBy?: string
}

/** The durable metadata returned after creating one worktree. */
export type CreatedWorktree = Omit<
  KindInput<(typeof import("../persistence/store.ts"))["db"]["schema"]["worktrees"]>,
  "sessionId"
>

/**
 * Resolves the first applicable worktree plugin for one repository.
 */
export async function resolveWorktreePlugin(options: WorktreeOptions) {
  assertGitRepository(options.cwd)

  for (const candidate of listWorktreePlugins(options)) {
    if (await candidate.isApplicable(options.cwd)) {
      return candidate
    }
  }

  return defaultPlugin
}

/**
 * Creates one worktree and reports which plugin ultimately handled the setup.
 */
export async function createWorktree(options: CreateWorktreeOptions) {
  const repoRoot = normalizeExistingPath(options.cwd)
  const requestedCwd = resolveRequestedCwd(repoRoot, options.requestedCwd)
  const plugin = await resolveWorktreePlugin({
    ...options,
    cwd: repoRoot,
  })
  const setupOptions = createSetupOptions({
    ...options,
    cwd: repoRoot,
  })

  if (plugin !== defaultPlugin) {
    try {
      const worktreeDir = await plugin.setup(setupOptions)
      if (worktreeDir) {
        return createWorktreeMetadata({
          repoRoot,
          requestedCwd,
          worktreeDir,
          branchName: options.branchName,
          poweredBy: plugin.name,
        })
      }
    } catch {
      // Suppress console output; default plugin handles fallback.
    }
  }

  let worktreeDir: string | null = null
  try {
    worktreeDir = await defaultPlugin.setup(setupOptions)
  } catch (err) {
    throw new Error(
      `Default worktree plugin failed to setup the workspace: ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    )
  }

  if (!worktreeDir) {
    throw new Error(`Default worktree plugin failed to setup the workspace (returned null).`)
  }

  return createWorktreeMetadata({
    repoRoot,
    requestedCwd,
    worktreeDir,
    branchName: options.branchName,
    poweredBy: defaultPlugin.name,
  })
}

/**
 * Deletes one worktree using the originating plugin when available and falls back to the default plugin.
 */
export async function deleteWorktree(options: DeleteWorktreeOptions) {
  const plugin = await resolveDeleteWorktreePlugin(options)
  if (await plugin.cleanup(options.worktreeDir, options.branchName)) {
    return true
  }

  if (plugin === defaultPlugin) {
    return false
  }

  return defaultPlugin.cleanup(options.worktreeDir, options.branchName)
}

/**
 * Lists the candidate plugins in evaluation order for one repository.
 */
function listWorktreePlugins(options: WorktreeOptions) {
  return [...(options.plugins || []), worktrunkPlugin, defaultPlugin]
}

/**
 * Resolves the plugin that should be used for worktree deletion.
 */
async function resolveDeleteWorktreePlugin(options: DeleteWorktreeOptions) {
  assertGitRepository(options.cwd)

  if (options.poweredBy) {
    const poweredByPlugin = listWorktreePlugins(options).find(
      (candidate) => candidate.name === options.poweredBy,
    )
    if (poweredByPlugin) {
      return poweredByPlugin
    }
  }

  return resolveWorktreePlugin(options)
}

/**
 * Builds the plugin setup payload from one public create request.
 */
function createSetupOptions(options: CreateWorktreeOptions) {
  return {
    cwd: options.cwd,
    branchName: options.branchName,
    defaultDirName: options.defaultPluginDirName,
  }
}

/**
 * Builds the persisted metadata shape shared by daemon session worktrees.
 */
function createWorktreeMetadata(params: {
  repoRoot: string
  requestedCwd: string
  worktreeDir: string
  branchName: string
  poweredBy: string
}) {
  const relativeCwd = path.relative(params.repoRoot, params.requestedCwd)
  const normalizedWorktreeDir = path.resolve(params.worktreeDir)

  return {
    repoRoot: params.repoRoot,
    requestedCwd: params.requestedCwd,
    effectiveCwd:
      relativeCwd.length === 0
        ? normalizedWorktreeDir
        : path.join(normalizedWorktreeDir, relativeCwd),
    worktreeDir: normalizedWorktreeDir,
    branchName: params.branchName,
    poweredBy: params.poweredBy,
  } satisfies CreatedWorktree
}

/**
 * Resolves one requested cwd and verifies that it stays inside the repository root.
 */
function resolveRequestedCwd(repoRoot: string, requestedCwd = repoRoot) {
  const normalizedRequestedCwd = normalizeExistingPath(requestedCwd)
  const relativeCwd = path.relative(repoRoot, normalizedRequestedCwd)

  if (relativeCwd.startsWith("..") || path.isAbsolute(relativeCwd)) {
    throw new Error(`Requested cwd must stay within the git repository: ${normalizedRequestedCwd}`)
  }

  return normalizedRequestedCwd
}

/**
 * Resolves one existing filesystem path to a stable canonical path when possible.
 */
function normalizeExistingPath(value: string) {
  const normalizedValue = path.resolve(value)

  try {
    return fs.realpathSync.native(normalizedValue)
  } catch {
    return normalizedValue
  }
}

/**
 * Verifies that one cwd points at a git repository before any plugin work runs.
 */
function assertGitRepository(cwd: string) {
  if (!fs.existsSync(path.join(cwd, ".git"))) {
    throw new Error(`Not a git repository: ${cwd}`)
  }
}
