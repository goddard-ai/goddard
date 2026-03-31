import * as fs from "node:fs"
import * as path from "node:path"
import { runCommand } from "./process.ts"
import type { WorktreePlugin, WorktreeSetupOptions } from "./types.ts"

export const worktrunkPlugin: WorktreePlugin = {
  name: "worktrunk",

  async isApplicable(cwd: string): Promise<boolean> {
    try {
      // Check if the current project is a valid worktrunk environment by checking for .config/wt.toml
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

  async setup(options: WorktreeSetupOptions): Promise<string | null> {
    try {
      const switchResult = await runCommand("wt", ["switch", options.branchName], {
        cwd: options.cwd,
        stdin: "ignore",
      })

      if (switchResult.status === 0) {
        // Find the newly created worktree path using git from within the project dir
        const worktreeListResult = await runCommand("git", ["worktree", "list"], {
          cwd: options.cwd,
        })

        if (worktreeListResult.status === 0) {
          const lines = worktreeListResult.stdout.split("\n")
          for (const line of lines) {
            if (line.includes(`[${options.branchName}]`)) {
              const wtPath = line.split(" ")[0]
              if (wtPath) {
                return wtPath
              }
            }
          }
        }
      }

      // Edge case: Worktrunk switch succeeded but we couldn't find the path.
      // Let's try to remove it before falling back so we don't leak it.
      if (switchResult.status === 0) {
        await runCommand("wt", ["remove", options.branchName], {
          cwd: options.cwd,
          stdin: "ignore",
        })
      }
    } catch {
      // Setup failed
    }

    return null
  },

  async cleanup(worktreeDir: string, branchName: string): Promise<boolean> {
    try {
      const result = await runCommand("wt", ["remove", branchName], {
        // Execute command from parent dir so we aren't inside the directory we're trying to delete
        cwd: path.dirname(worktreeDir) || "/",
        stdin: "ignore",
      })

      if (result.status === 0) {
        return true
      }
    } catch {
      // Cleanup failed
    }

    return false
  },
}
