import { spawnSync } from "node:child_process"
import * as fs from "node:fs"
import * as path from "node:path"
import * as os from "node:os"
import * as crypto from "node:crypto"
import type { WorktreePlugin, WorktreeSetupOptions } from "./types.js"

export const defaultPlugin: WorktreePlugin = {
  name: "default",

  isApplicable(): boolean {
    return true
  },

  setup(options: WorktreeSetupOptions): string | null {
    let agentsDirPath: string

    if (options.defaultDirName) {
      agentsDirPath = path.join(options.cwd, options.defaultDirName)
    } else if (fs.existsSync(path.join(options.cwd, ".worktrees"))) {
      agentsDirPath = path.join(options.cwd, ".worktrees")
    } else if (fs.existsSync(path.join(options.cwd, "worktrees"))) {
      agentsDirPath = path.join(options.cwd, "worktrees")
    } else {
      // Use system temp directory if no specific directory is present, appending hash to prevent collisions
      const hash = crypto.createHash("sha256").update(options.cwd).digest("hex").substring(0, 7)
      agentsDirPath = path.join(os.tmpdir(), "goddard-worktrees", `${path.basename(options.cwd)}-${hash}`)
    }

    const worktreeDir = path.join(agentsDirPath, `${options.branchName}-${Date.now()}`)

    if (!fs.existsSync(agentsDirPath)) {
      spawnSync("mkdir", ["-p", agentsDirPath])
    }

    // Use copy-on-write clone to create the workspace instantly based on OS
    try {
      let cpArgs = ["-R", options.cwd + "/", worktreeDir]
      if (process.platform === "darwin") {
        cpArgs = ["-cR", options.cwd + "/", worktreeDir]
      } else if (process.platform === "linux") {
        cpArgs = ["--reflink=auto", "-R", options.cwd + "/", worktreeDir]
      }

      let cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" })

      if (cloneResult.status !== 0 && process.platform === "darwin") {
        // Fallback to regular copy if APFS clone fails on macOS
        cpArgs = ["-R", options.cwd + "/", worktreeDir]
        cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" })
      }

      if (cloneResult.status !== 0) {
        throw new Error(`cp command exited with code ${cloneResult.status}`)
      }
    } catch (err) {
      if (err instanceof Error && err.message.includes("cp command exited with code")) {
        throw err
      }
      throw new Error(
        `Failed to create workspace: ${err instanceof Error ? err.message : String(err)}`,
        { cause: err },
      )
    }

    // Fetch and checkout the branch in the new workspace
    try {
      const prNumberMatch = options.branchName.match(/^pr-(\d+)$/)
      if (prNumberMatch) {
        spawnSync("git", ["fetch", "origin", `pull/${prNumberMatch[1]}/head:${options.branchName}`], {
          cwd: worktreeDir,
          stdio: "ignore",
        })
      }

      spawnSync("git", ["checkout", options.branchName], {
        cwd: worktreeDir,
        stdio: "ignore",
      })
    } catch {
      // Ignore error
    }

    return worktreeDir
  },

  cleanup(worktreeDir: string): boolean {
    try {
      spawnSync("rm", ["-rf", worktreeDir], {
        encoding: "utf8",
        stdio: "ignore",
      })
      return true
    } catch {
      return false
    }
  },
}
