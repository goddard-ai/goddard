import { worktrunkPlugin } from "./worktrunk.js"
import { defaultPlugin } from "./default-plugin.js"
import type { WorktreePlugin, WorktreeSetupOptions } from "./types.js"
import * as fs from "node:fs"
import * as path from "node:path"

export type { WorktreePlugin, WorktreeSetupOptions }

export interface WorktreeOptions {
  plugins?: WorktreePlugin[]
  cwd: string
  defaultPluginDirName?: string
}

export class Worktree {
  readonly cwd: string
  readonly defaultPluginDirName?: string
  plugin: WorktreePlugin

  constructor(options: WorktreeOptions) {
    this.cwd = options.cwd
    this.defaultPluginDirName = options.defaultPluginDirName

    if (!fs.existsSync(path.join(this.cwd, ".git"))) {
      throw new Error(`Not a git repository: ${this.cwd}`)
    }

    const candidates = [...(options.plugins || []), worktrunkPlugin]
    this.plugin = candidates.find((p) => p.isApplicable(this.cwd)) || defaultPlugin
  }

  get poweredBy(): string {
    return this.plugin.name
  }

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

  cleanup(worktreeDir: string, branchName: string): void {
    this.plugin.cleanup(worktreeDir, branchName)
  }
}
