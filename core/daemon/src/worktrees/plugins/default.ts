/** Default worktree plugin that clones or creates git worktrees under daemon control. */
import type { WorktreePlugin, WorktreeSetupOptions } from "@goddard-ai/worktree-plugin"
import * as crypto from "node:crypto"
import * as fs from "node:fs"
import { constants as fsConstants } from "node:fs"
import { cp, mkdir, rm } from "node:fs/promises"
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

    try {
      let cloneSucceeded = await cloneWorkspace(options.cwd, worktreeDir)

      if (!cloneSucceeded) {
        const wtResult = await runCommand("git", ["worktree", "add", "--detach", worktreeDir], {
          cwd: options.cwd,
          stdin: "ignore",
        })

        if (wtResult.status !== 0) {
          cloneSucceeded = await cloneWorkspace(options.cwd, worktreeDir, false)

          if (!cloneSucceeded) {
            throw new Error(`Workspace copy failed for ${worktreeDir}`)
          }
        }
      }
    } catch (err) {
      if (err instanceof Error && err.message.includes("Workspace copy failed")) {
        throw err
      }
      throw new Error(
        `Failed to create workspace: ${err instanceof Error ? err.message : String(err)}`,
        { cause: err },
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
 * Copies one repository into a new worktree directory, preferring copy-on-write when supported.
 */
async function cloneWorkspace(sourceDir: string, targetDir: string, preferCopyOnWrite = true) {
  try {
    await cp(sourceDir, targetDir, {
      recursive: true,
      mode: preferCopyOnWrite ? reflinkModeForPlatform() : 0,
    })
    return true
  } catch {
    return false
  }
}

/**
 * Returns the `fs.cp` mode used to request reflink copies on platforms that support them.
 */
function reflinkModeForPlatform() {
  if (process.platform === "darwin" || process.platform === "linux") {
    return fsConstants.COPYFILE_FICLONE
  }

  return 0
}

/**
 * Resolves the home directory used for global worktree storage in a testable way.
 */
function resolveHomeDir() {
  return process.env.HOME || os.homedir()
}
