/** Worktrunk-backed worktree plugin used when a repository is managed by `wt`. */
import * as fs from "node:fs"
import * as path from "node:path"
import { runGitCommand } from "@goddard-ai/git"
import type { WorktreePlugin, WorktreeSetupOptions } from "@goddard-ai/worktree-plugin"

import { runCommand } from "../process.ts"

export const worktrunkPlugin: WorktreePlugin = {
  name: "worktrunk",

  async isApplicable(cwd: string) {
    try {
      if (!fs.existsSync(path.join(cwd, ".config", "wt.toml"))) {
        return false
      }

      const versionResult = await runCommand("wt", ["--version"], {
        stdin: "ignore",
      })
      return versionResult.status === 0
    } catch {
      return false
    }
  },

  async setup(options: WorktreeSetupOptions) {
    try {
      const switchResult = await runCommand("wt", ["switch", options.branchName], {
        cwd: options.cwd,
        stdin: "ignore",
      })

      if (switchResult.status === 0) {
        const worktreeListResult = await runGitCommand(options.cwd, ["worktree", "list"])

        if (worktreeListResult.status === 0) {
          const lines = worktreeListResult.stdout.split("\n")
          for (const line of lines) {
            if (line.includes(`[${options.branchName}]`)) {
              const wtPath = line.split(" ")[0]
              if (wtPath) {
                if (options.baseBranchName) {
                  await runGitCommand(
                    wtPath,
                    ["checkout", "-B", options.branchName, options.baseBranchName],
                    { stdin: "ignore" },
                  )
                }

                return wtPath
              }
            }
          }
        }
      }

      if (switchResult.status === 0) {
        await runCommand("wt", ["remove", options.branchName], {
          cwd: options.cwd,
          stdin: "ignore",
        })
      }
    } catch {
      // Setup failed.
    }

    return null
  },

  async cleanup(options) {
    try {
      const result = await runCommand("wt", ["remove", options.branchName], {
        cwd: path.dirname(options.worktreeDir) || "/",
        stdin: "ignore",
      })

      if (result.status === 0) {
        return true
      }
    } catch {
      // Cleanup failed.
    }

    return false
  },
}
