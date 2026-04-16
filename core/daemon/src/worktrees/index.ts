/** Daemon-owned worktree creation and cleanup helpers with pluggable strategies. */
import type { DaemonWorktree } from "@goddard-ai/schema/daemon/store"
import type { WorktreePlugin, WorktreeSetupOptions } from "@goddard-ai/worktree-plugin"
import * as fs from "node:fs"
import * as path from "node:path"

import { defaultPlugin } from "./plugins/default.ts"
import { worktrunkPlugin } from "./plugins/worktrunk.ts"
import { runCommand } from "./process.ts"

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
export type CreatedWorktree = Omit<DaemonWorktree, "id" | "sessionId">

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
 * Creates one linked git worktree and reports which plugin ultimately handled the setup.
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
        return await createWorktreeMetadata({
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

  return await createWorktreeMetadata({
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
async function createWorktreeMetadata(params: {
  repoRoot: string
  requestedCwd: string
  worktreeDir: string
  branchName: string
  poweredBy: string
}) {
  const normalizedRepoRoot = normalizeExistingPath(params.repoRoot)
  const normalizedWorktreeDir = normalizeExistingPath(params.worktreeDir)
  await assertLinkedWorktree({
    repoRoot: normalizedRepoRoot,
    worktreeDir: normalizedWorktreeDir,
    poweredBy: params.poweredBy,
  })

  const relativeCwd = path.relative(normalizedRepoRoot, params.requestedCwd)

  return {
    repoRoot: normalizedRepoRoot,
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
 * Verifies that one plugin-produced directory is a linked worktree attached to the source repository.
 */
async function assertLinkedWorktree(params: {
  repoRoot: string
  worktreeDir: string
  poweredBy: string
}) {
  if (params.repoRoot === params.worktreeDir) {
    throw new Error(
      `Worktree plugin "${params.poweredBy}" returned the repository root instead of a linked worktree: ${params.worktreeDir}`,
    )
  }

  const [repoCommonDir, worktreeCommonDir, worktreeGitDir] = await Promise.all([
    resolveGitCommonDir(params.repoRoot),
    resolveGitCommonDir(params.worktreeDir),
    resolveGitDir(params.worktreeDir),
  ])

  if (
    !repoCommonDir ||
    !worktreeCommonDir ||
    !worktreeGitDir ||
    repoCommonDir !== worktreeCommonDir ||
    worktreeGitDir === worktreeCommonDir
  ) {
    throw new Error(
      `Worktree plugin "${params.poweredBy}" must create a linked git worktree for ${params.worktreeDir}`,
    )
  }
}

/**
 * Resolves one repository's git dir as an absolute path when available.
 */
async function resolveGitDir(cwd: string) {
  const result = await runCommand("git", ["rev-parse", "--git-dir"], {
    cwd,
    stdin: "ignore",
  })

  if (result.status !== 0) {
    return null
  }

  const gitDir = result.stdout.trim()
  return gitDir ? normalizeExistingPath(path.resolve(cwd, gitDir)) : null
}

/**
 * Resolves one repository's common git dir as an absolute path when available.
 */
async function resolveGitCommonDir(cwd: string) {
  const result = await runCommand("git", ["rev-parse", "--git-common-dir"], {
    cwd,
    stdin: "ignore",
  })

  if (result.status !== 0) {
    return null
  }

  const commonDir = result.stdout.trim()
  return commonDir ? normalizeExistingPath(path.resolve(cwd, commonDir)) : null
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
