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
   * The default directory name to use for created worktrees.
   */
  defaultDirName?: string
}

/**
 * A plugin that defines how worktrees should be managed.
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
   * Sets up a new worktree.
   */
  setup(options: WorktreeSetupOptions): Promise<string | null>

  /**
   * Cleans up an existing worktree.
   */
  cleanup(worktreeDir: string, branchName: string): Promise<boolean>
}
