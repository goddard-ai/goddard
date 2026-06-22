/** Default worktree plugin that creates linked git worktrees for sessions. */
import * as crypto from "node:crypto"
import * as fs from "node:fs"
import { mkdir, rm } from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { createGitHost } from "@goddard-ai/libgit2"
import type {
  WorktreeCleanupOptions,
  WorktreePlugin,
  WorktreeSetupOptions,
} from "@goddard-ai/worktree-plugin"

import {
  addDetachedWorktree,
  checkoutBranchFromFetchHead,
  checkoutExistingBranch,
  checkoutWorktreeBranch,
  fetchPullRequestHead,
  removeWorktree,
} from "../../git/worktrees.ts"

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

    await addDetachedWorktree(options.cwd, worktreeDir)

    try {
      const prNumberMatch = options.branchName.match(/(?:^pr-|\/pr\/)(\d+)$/)
      if (prNumberMatch) {
        await fetchPullRequestHead({
          worktreeDir,
          prNumber: prNumberMatch[1],
          branchName: options.branchName,
        })
        await checkoutBranchFromFetchHead(worktreeDir, options.branchName)
        return worktreeDir
      }

      await checkoutWorktreeBranch({
        worktreeDir,
        branchName: options.branchName,
        baseBranchName: options.baseBranchName,
      })
    } catch {
      try {
        await checkoutExistingBranch(worktreeDir, options.branchName)
      } catch {
        // Ignore error.
      }
    }

    return worktreeDir
  },

  async cleanup(options: WorktreeCleanupOptions) {
    try {
      if (!(await removeWorktree(options.cwd, options.worktreeDir))) {
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
