/**
 * Options for setting up a worktree.
 */
export interface WorktreeSetupOptions {
  /**
   * The current working directory of the base repository.
   */
  cwd: string
  /**
   * The name of the branch to set up a worktree for.
   */
  branchName: string
  /**
   * Optional default directory name to use for the worktree if applicable.
   */
  defaultDirName?: string
}

/**
 * Interface defining a strategy/plugin for creating and managing worktrees.
 */
export interface WorktreePlugin {
  /**
   * The name of the plugin.
   */
  name: string
  /**
   * Checks whether this plugin is applicable for the given working directory.
   *
   * @param cwd The current working directory.
   * @returns `true` if applicable, `false` otherwise.
   */
  isApplicable(cwd: string): boolean
  /**
   * Sets up a worktree based on the provided options.
   *
   * @param options Options for setting up the worktree.
   * @returns The path to the created worktree, or `null` if setup failed.
   */
  setup(options: WorktreeSetupOptions): string | null
  /**
   * Cleans up the worktree and associated resources.
   *
   * @param worktreeDir The path to the worktree to clean up.
   * @param branchName The name of the branch associated with the worktree.
   * @returns `true` if cleanup was successful, `false` otherwise.
   */
  cleanup(worktreeDir: string, branchName: string): boolean
}
