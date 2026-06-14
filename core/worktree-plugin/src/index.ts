/** Shared plugin types for daemon-owned and third-party worktree integrations. */

/**
 * Options passed to a plugin when setting up a worktree.
 */
export interface WorktreeSetupOptions {
  /**
   * The current working directory of the original repository.
   */
  cwd: string

  /**
   * The name of the branch to create a worktree for.
   */
  branchName: string

  /**
   * Optional existing branch to seed the new worktree branch from.
   */
  baseBranchName?: string

  /**
   * The default directory name to use for created worktrees.
   */
  defaultDirName?: string
}

/**
 * Options passed to a plugin when cleaning up a worktree.
 */
export interface WorktreeCleanupOptions {
  /**
   * The current working directory of the original repository.
   */
  cwd: string

  /**
   * The directory path of the worktree to clean up.
   */
  worktreeDir: string

  /**
   * The branch associated with the worktree.
   */
  branchName: string
}

/**
 * A plugin that defines how linked git worktrees should be managed.
 */
export interface WorktreePlugin {
  /**
   * The name of the plugin.
   */
  name: string

  /**
   * Determines whether this plugin is applicable for the given environment.
   */
  isApplicable(cwd: string): boolean | Promise<boolean>

  /**
   * Sets up one linked git worktree and returns its directory path.
   */
  setup(options: WorktreeSetupOptions): Promise<string | null>

  /**
   * Cleans up an existing worktree.
   */
  cleanup(options: WorktreeCleanupOptions): Promise<boolean>
}
