/** Default worktree plugin that creates linked git worktrees for sessions. */
import * as crypto from "node:crypto"
import * as fs from "node:fs"
import { mkdir, rm } from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { createGitHost } from "@goddard-ai/git"
import type {
  WorktreeCleanupOptions,
  WorktreePlugin,
  WorktreeSetupOptions,
} from "@goddard-ai/worktree-plugin"

import { runGitCommand } from "../../git-command.ts"

export const defaultPlugin: WorktreePlugin = {
  name: "default",

  isApplicable() {
    return true
  },

  async setup(options: WorktreeSetupOptions) {
    let agentsDirPath: string
    const bareRepository = await isBareRepository(options.cwd)

    if (options.defaultDirName) {
      agentsDirPath = path.join(options.cwd, options.defaultDirName)
    } else if (!bareRepository && fs.existsSync(path.join(options.cwd, ".worktrees"))) {
      agentsDirPath = path.join(options.cwd, ".worktrees")
    } else if (!bareRepository && fs.existsSync(path.join(options.cwd, "worktrees"))) {
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
    await mkdir(path.dirname(worktreeDir), { recursive: true })

    const wtResult = await runGitCommand(
      options.cwd,
      ["worktree", "add", "--detach", worktreeDir],
      {
        stdin: "ignore",
      },
    )

    if (wtResult.status !== 0) {
      throw new Error(
        `Failed to create linked worktree at ${worktreeDir}: ${wtResult.stderr.trim() || wtResult.stdout.trim() || "git worktree add exited unsuccessfully"}`,
      )
    }

    try {
      const prNumberMatch = options.branchName.match(/(?:^pr-|\/pr\/)(\d+)$/)
      if (prNumberMatch) {
        await runGitCommand(
          worktreeDir,
          ["fetch", "origin", `pull/${prNumberMatch[1]}/head:${options.branchName}`],
          { stdin: "ignore" },
        )

        await runGitCommand(worktreeDir, ["checkout", "-B", options.branchName, "FETCH_HEAD"], {
          stdin: "ignore",
        })
        return worktreeDir
      }

      await runGitCommand(
        worktreeDir,
        options.baseBranchName
          ? ["checkout", "-B", options.branchName, options.baseBranchName]
          : ["checkout", "-B", options.branchName],
        { stdin: "ignore" },
      )
    } catch {
      try {
        await runGitCommand(worktreeDir, ["checkout", options.branchName], {
          stdin: "ignore",
        })
      } catch {
        // Ignore error.
      }
    }

    return worktreeDir
  },

  async cleanup(options: WorktreeCleanupOptions) {
    try {
      const wtResult = await runGitCommand(
        options.cwd,
        ["worktree", "remove", "--force", options.worktreeDir],
        { stdin: "ignore" },
      )

      if (wtResult.status !== 0) {
        await rm(options.worktreeDir, { recursive: true, force: true })
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

async function isBareRepository(cwd: string) {
  return await createGitHost().repository.isBareRepository(cwd)
}
