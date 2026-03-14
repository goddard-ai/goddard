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
  readonly plugin: WorktreePlugin

  constructor(options: WorktreeOptions) {
    this.projectDir = options.projectDir

    const candidates = [...(options.plugins || []), worktrunkPlugin, defaultPlugin]

    const selected = candidates.find((p) => p.isApplicable(this.projectDir))
    if (!selected) {
      throw new Error("No applicable worktree plugin found.")
    }

    this.plugin = selected
  }

  setup(prNumber: number): { worktreeDir: string; branchName: string; isWorktrunk: boolean } {
    const branchName = `pr-${prNumber}`
    const worktreeDir = this.plugin.setup(this.projectDir, prNumber, branchName)

    if (!worktreeDir) {
      throw new Error(`Plugin ${this.plugin.name} failed to setup worktree.`)
    }

    return {
      worktreeDir,
      branchName,
      isWorktrunk: this.plugin.name === "worktrunk",
    }
  }

  cleanup(worktreeDir: string, branchName: string): void {
    this.plugin.cleanup(worktreeDir, branchName)
  }
}
