/** Default worktree plugin that creates linked git worktrees under daemon control. */
import type { WorktreePlugin, WorktreeSetupOptions } from "@goddard-ai/worktree-plugin"
import * as crypto from "node:crypto"
import * as fs from "node:fs"
import { mkdir, rm } from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"

import { runCommand } from "../process.ts"

export const defaultPlugin: WorktreePlugin = {
  name: "default",

  isApplicable() {
    return true
  },

  async setup(options: WorktreeSetupOptions) {
    let agentsDirPath: string

    if (options.defaultDirName) {
      agentsDirPath = path.join(options.cwd, options.defaultDirName)
    } else if (fs.existsSync(path.join(options.cwd, ".worktrees"))) {
      agentsDirPath = path.join(options.cwd, ".worktrees")
    } else if (fs.existsSync(path.join(options.cwd, "worktrees"))) {
      agentsDirPath = path.join(options.cwd, "worktrees")
    } else {
      const hash = crypto.createHash("sha256").update(options.cwd).digest("hex").substring(0, 7)
      agentsDirPath = path.join(
        resolveHomeDir(),
        ".goddard",
        "worktrees",
        `${path.basename(options.cwd)}-${hash}`,
      )
    }

    const worktreeDir = path.join(agentsDirPath, `${options.branchName}-${Date.now()}`)

    if (!fs.existsSync(agentsDirPath)) {
      await mkdir(agentsDirPath, { recursive: true })
    }

    const wtResult = await runCommand("git", ["worktree", "add", "--detach", worktreeDir], {
      cwd: options.cwd,
      stdin: "ignore",
    })

    if (wtResult.status !== 0) {
      throw new Error(
        `Failed to create linked worktree at ${worktreeDir}: ${wtResult.stderr.trim() || wtResult.stdout.trim() || "git worktree add exited unsuccessfully"}`,
      )
    }

    try {
      const prNumberMatch = options.branchName.match(/^pr-(\d+)$/)
      if (prNumberMatch) {
        await runCommand(
          "git",
          ["fetch", "origin", `pull/${prNumberMatch[1]}/head:${options.branchName}`],
          {
            cwd: worktreeDir,
            stdin: "ignore",
          },
        )
      }

      await runCommand("git", ["checkout", options.branchName], {
        cwd: worktreeDir,
        stdin: "ignore",
      })
    } catch {
      // Ignore error.
    }

    return worktreeDir
  },

  async cleanup(worktreeDir: string, _branchName: string) {
    try {
      const wtResult = await runCommand("git", ["worktree", "remove", "--force", worktreeDir], {
        stdin: "ignore",
      })

      if (wtResult.status !== 0) {
        await rm(worktreeDir, { recursive: true, force: true })
      }
      return true
    } catch {
      return false
    }
  },
}

/**
 * Resolves the home directory used for global worktree storage in a testable way.
 */
function resolveHomeDir() {
  return process.env.HOME || os.homedir()
}
