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
   *
   * @param cwd - The current working directory of the original repository.
   * @returns `true` if the plugin is applicable, `false` otherwise.
   */
  isApplicable(cwd: string): boolean

  /**
   * Sets up a new worktree.
   *
   * @param options - Options for setting up the worktree.
   * @returns The path to the created worktree directory, or `null` if setup failed.
   */
  setup(options: WorktreeSetupOptions): string | null

  /**
   * Cleans up an existing worktree.
   *
   * @param worktreeDir - The path to the worktree directory to clean up.
   * @param branchName - The name of the branch associated with the worktree.
   * @returns `true` if the cleanup was successful, `false` otherwise.
   */
  cleanup(worktreeDir: string, branchName: string): boolean
}
