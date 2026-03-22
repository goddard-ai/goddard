import { spawnSync } from "node:child_process"
import * as fs from "node:fs"
import * as path from "node:path"
import type { WorktreePlugin } from "./types.ts"

export const worktrunkPlugin: WorktreePlugin = {
  name: "worktrunk",

  isApplicable(cwd: string): boolean {
    try {
      // Check if the current project is a valid worktrunk environment by checking for .config/wt.toml
      if (!fs.existsSync(path.join(cwd, ".config", "wt.toml"))) {
        return false
      }

      const versionResult = spawnSync("wt", ["--version"])
      return versionResult.status === 0
    } catch {
      return false
    }
  },

  setup(options): string | null {
    try {
      const switchResult = spawnSync("wt", ["switch", options.branchName], {
        cwd: options.cwd,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      })

      if (switchResult.status === 0) {
        // Find the newly created worktree path using git from within the project dir
        const worktreeListResult = spawnSync("git", ["worktree", "list"], {
          cwd: options.cwd,
          encoding: "utf8",
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
        spawnSync("wt", ["remove", options.branchName], {
          cwd: options.cwd,
          encoding: "utf8",
          stdio: "ignore",
        })
      }
    } catch {
      // Setup failed
    }

    return null
  },

  cleanup(worktreeDir: string, branchName: string): boolean {
    try {
      const result = spawnSync("wt", ["remove", branchName], {
        // Execute command from parent dir so we aren't inside the directory we're trying to delete
        cwd: path.dirname(worktreeDir) || "/",
        encoding: "utf8",
        stdio: "ignore",
      })

      if (result.status === 0 && !result.error) {
        return true
      }
    } catch {
      // Cleanup failed
    }

    return false
  },
}
