import { worktrunkPlugin } from "./worktrunk.js"
import { defaultPlugin } from "./default-plugin.js"
import type { WorktreePlugin } from "./types.js"

export type { WorktreePlugin }

export interface WorktreeOptions {
  plugins?: WorktreePlugin[]
  projectDir: string
}

export class Worktree {
  readonly projectDir: string
  plugin: WorktreePlugin
  private readonly candidates: WorktreePlugin[]

  constructor(options: WorktreeOptions) {
    this.projectDir = options.projectDir

    this.candidates = [...(options.plugins || []), worktrunkPlugin]
    this.plugin = this.candidates.find((p) => p.isApplicable(this.projectDir)) || defaultPlugin
  }

  setup(prNumber: number): { worktreeDir: string; branchName: string; isWorktrunk: boolean } {
    const branchName = `pr-${prNumber}`

    // Evaluate the initially selected custom plugin or worktrunkPlugin
    if (this.plugin !== defaultPlugin) {
      let worktreeDir: string | null = null
      try {
        worktreeDir = this.plugin.setup(this.projectDir, prNumber, branchName)
      } catch (err) {
        console.error(`[WARN] Plugin ${this.plugin.name} threw an error during setup. Falling back to default plugin.`)
      }

      if (worktreeDir) {
        return {
          worktreeDir,
          branchName,
          isWorktrunk: this.plugin.name === "worktrunk",
        }
      }

      console.warn(`[WARN] Plugin ${this.plugin.name} failed to setup worktree (returned null). Falling back to default plugin.`)

      // Since it failed, permanently change the active plugin to defaultPlugin for cleanup
      this.plugin = defaultPlugin
    }

    // Evaluate the default fallback
    let worktreeDir: string | null = null
    try {
      worktreeDir = defaultPlugin.setup(this.projectDir, prNumber, branchName)
    } catch {
       // Intentionally left blank as default plugin logs its own errors
    }

    if (!worktreeDir) {
      throw new Error(`Default worktree plugin failed to setup the workspace.`)
    }

    return {
      worktreeDir,
      branchName,
      isWorktrunk: false,
    }
  }

  cleanup(worktreeDir: string, branchName: string): void {
    this.plugin.cleanup(worktreeDir, branchName)
  }
}
