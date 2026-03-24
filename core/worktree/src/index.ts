import * as fs from "node:fs"
import * as path from "node:path"
import { defaultPlugin } from "./default-plugin.ts"
import type { WorktreePlugin, WorktreeSetupOptions } from "./types.ts"
import { worktrunkPlugin } from "./worktrunk.ts"

export type { WorktreePlugin, WorktreeSetupOptions }

/**
 * Configuration options for initializing a Worktree instance.
 */
export interface WorktreeOptions {
  /**
   * Optional list of plugins to consider for worktree management.
   */
  plugins?: WorktreePlugin[]
  /**
   * The current working directory representing the base repository.
   */
  cwd: string
  /**
   * Optional default directory name to use when setting up worktrees.
   */
  defaultPluginDirName?: string
}

/**
 * A utility class for creating and managing Git worktrees with pluggable strategies.
 */
export class Worktree {
  /**
   * The current working directory of the base repository.
   */
  readonly cwd: string
  /**
   * The default directory name to use when setting up worktrees.
   */
  readonly defaultPluginDirName?: string
  /**
   * The active worktree management plugin.
   */
  plugin: WorktreePlugin

  /**
   * Creates a new Worktree instance.
   *
   * @param options Configuration options.
   * @throws {Error} If the provided `cwd` is not a valid git repository.
   */
  constructor(options: WorktreeOptions) {
    this.cwd = options.cwd
    this.defaultPluginDirName = options.defaultPluginDirName

    if (!fs.existsSync(path.join(this.cwd, ".git"))) {
      throw new Error(`Not a git repository: ${this.cwd}`)
    }

    const candidates = [...(options.plugins || []), worktrunkPlugin]
    this.plugin = candidates.find((p) => p.isApplicable(this.cwd)) || defaultPlugin
  }

  /**
   * Gets the name of the plugin powering the current Worktree instance.
   */
  get poweredBy(): string {
    return this.plugin.name
  }

  /**
   * Sets up a new worktree for the specified branch.
   *
   * @param branchName The name of the branch to set up.
   * @returns An object containing the created `worktreeDir` path and `branchName`.
   * @throws {Error} If the worktree setup fails.
   */
  setup(branchName: string): { worktreeDir: string; branchName: string } {
    const setupOptions: WorktreeSetupOptions = {
      cwd: this.cwd,
      branchName,
      defaultDirName: this.defaultPluginDirName,
    }

    // Evaluate the initially selected custom plugin or worktrunkPlugin
    if (this.plugin !== defaultPlugin) {
      let worktreeDir: string | null = null
      try {
        worktreeDir = this.plugin.setup(setupOptions)
      } catch {
        // Suppress console output; default plugin handles fallback
      }

      if (worktreeDir) {
        return {
          worktreeDir,
          branchName,
        }
      }

      // Since it failed, permanently change the active plugin to defaultPlugin for cleanup
      this.plugin = defaultPlugin
    }

    // Evaluate the default fallback
    let worktreeDir: string | null = null
    try {
      worktreeDir = defaultPlugin.setup(setupOptions)
    } catch (err) {
      throw new Error(
        `Default worktree plugin failed to setup the workspace: ${err instanceof Error ? err.message : String(err)}`,
        { cause: err },
      )
    }

    if (!worktreeDir) {
      throw new Error(`Default worktree plugin failed to setup the workspace (returned null).`)
    }

    return {
      worktreeDir,
      branchName,
    }
  }

  /**
   * Cleans up the specified worktree.
   *
   * @param worktreeDir The path to the worktree to clean up.
   * @param branchName The branch associated with the worktree.
   * @returns `true` if the cleanup was successful, `false` otherwise.
   */
  cleanup(worktreeDir: string, branchName: string): boolean {
    return this.plugin.cleanup(worktreeDir, branchName)
  }
}
