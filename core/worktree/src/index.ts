import * as fs from "node:fs"
import * as path from "node:path"
import { defaultPlugin } from "./default-plugin.ts"
import type { WorktreePlugin, WorktreeSetupOptions } from "./types.ts"
import { worktrunkPlugin } from "./worktrunk.ts"

export type { WorktreePlugin, WorktreeSetupOptions }

/**
 * Options for configuring a Worktree instance.
 */
export interface WorktreeOptions {
  /**
   * Optional list of plugins to use for worktree management.
   * If provided, these will be evaluated before the default plugin.
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
 * A utility class for managing Git worktrees with pluggable strategies.
 */
export class Worktree {
  /**
   * The current working directory of the original repository.
   */
  readonly cwd: string

  /**
   * The default directory name to use for created worktrees.
   */
  readonly defaultPluginDirName?: string

  /**
   * Candidate plugins evaluated before falling back to the default plugin.
   */
  readonly plugins: WorktreePlugin[]

  /**
   * The active plugin being used to manage worktrees.
   */
  plugin: WorktreePlugin

  /**
   * Tracks whether plugin applicability has already been resolved.
   */
  private pluginResolved = false

  /**
   * Creates a new Worktree instance.
   *
   * @param options - Configuration options for the Worktree.
   * @throws {Error} If the provided `cwd` is not a valid git repository.
   */
  constructor(options: WorktreeOptions) {
    this.cwd = options.cwd
    this.defaultPluginDirName = options.defaultPluginDirName

    if (!fs.existsSync(path.join(this.cwd, ".git"))) {
      throw new Error(`Not a git repository: ${this.cwd}`)
    }

    this.plugins = [...(options.plugins || []), worktrunkPlugin]
    this.plugin = defaultPlugin
  }

  /**
   * The name of the active plugin powering the worktree management.
   */
  get poweredBy(): string {
    return this.plugin.name
  }

  /**
   * Sets up a new worktree for the specified branch.
   *
   * @param branchName - The name of the branch to create a worktree for.
   * @returns An object containing the path to the created worktree directory and the branch name.
   * @throws {Error} If the default plugin fails to set up the workspace.
   */
  async setup(branchName: string): Promise<{ worktreeDir: string; branchName: string }> {
    const setupOptions: WorktreeSetupOptions = {
      cwd: this.cwd,
      branchName,
      defaultDirName: this.defaultPluginDirName,
    }

    await this.ensurePlugin()

    // Evaluate the initially selected custom plugin or worktrunkPlugin
    if (this.plugin !== defaultPlugin) {
      let worktreeDir: string | null = null
      try {
        worktreeDir = await this.plugin.setup(setupOptions)
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

    return {
      worktreeDir,
      branchName,
    }
  }

  /**
   * Cleans up an existing worktree.
   *
   * @param worktreeDir - The path to the worktree directory to clean up.
   * @param branchName - The name of the branch associated with the worktree.
   * @returns `true` if the cleanup was successful, `false` otherwise.
   */
  async cleanup(worktreeDir: string, branchName: string): Promise<boolean> {
    await this.ensurePlugin()
    return this.plugin.cleanup(worktreeDir, branchName)
  }

  /**
   * Resolves the first applicable plugin once and reuses it for later operations.
   */
  private async ensurePlugin(): Promise<void> {
    if (this.pluginResolved) {
      return
    }

    for (const candidate of this.plugins) {
      if (await candidate.isApplicable(this.cwd)) {
        this.plugin = candidate
        this.pluginResolved = true
        return
      }
    }

    this.plugin = defaultPlugin
    this.pluginResolved = true
  }
}
