import { spawnSync } from "node:child_process"
import type { WorktreePlugin } from "./types.js"

export const defaultPlugin: WorktreePlugin = {
  name: "default",

  isApplicable(): boolean {
    return true
  },

  setup(projectDir: string, prNumber: number, branchName: string): string | null {
    const agentsDir = `${projectDir}/.goddard-agents`
    const worktreeDir = `${agentsDir}/${branchName}-${Date.now()}`

    // Ensure agents dir exists
    spawnSync("mkdir", ["-p", agentsDir])

    // Use copy-on-write clone to create the workspace instantly based on OS
    try {
      let cpArgs = ["-R", projectDir + "/", worktreeDir]
      if (process.platform === "darwin") {
        cpArgs = ["-cR", projectDir + "/", worktreeDir]
      } else if (process.platform === "linux") {
        cpArgs = ["--reflink=auto", "-R", projectDir + "/", worktreeDir]
      }

      let cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" })

      if (cloneResult.status !== 0 && process.platform === "darwin") {
        // Fallback to regular copy if APFS clone fails on macOS
        cpArgs = ["-R", projectDir + "/", worktreeDir]
        cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" })
      }

      if (cloneResult.status !== 0) {
        throw new Error(`Cannot proceed with one-shot pi session. Aborting.`)
      }
    } catch (err) {
      if (err instanceof Error && err.message.includes("Cannot proceed")) {
        throw err
      }
      throw new Error(
        `Failed to create workspace: ${err instanceof Error ? err.message : String(err)}`,
        { cause: err },
      )
    }

    // Fetch and checkout the branch in the new workspace
    try {
      spawnSync("git", ["fetch", "origin", `pull/${prNumber}/head:${branchName}`], {
        cwd: worktreeDir,
        stdio: "ignore",
      })
      spawnSync("git", ["checkout", branchName], {
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
