import { GitCommandError, GitNotRepositoryError } from "../errors.ts"
import { normalizePath, resolveGitOutputPath } from "../paths.ts"
import type { GitHost, GitRunOptions } from "../types.ts"
import { runGitCommand } from "./command.ts"
import { parseGitStashes, parseGitWorktrees } from "./parsers.ts"

export function createCliGitHost(): GitHost {
  const run = async (cwd: string, args: string[], options: GitRunOptions = {}) => {
    const result = await runGitCommand(cwd, args, options)
    if (result.status !== 0 && options.allowFailure !== true) {
      throw new GitCommandError(cwd, args, result)
    }

    return result
  }

  return {
    repository: {
      resolveRoot: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--show-toplevel"], {
          allowFailure: true,
        })
        if (result.status !== 0 || !result.stdout.trim()) {
          throw new GitNotRepositoryError(cwd)
        }
        return await normalizePath(result.stdout.trim())
      },
      resolveGitDir: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--git-dir"])
        return await normalizePath(resolveGitOutputPath(cwd, result.stdout.trim()))
      },
      resolveCommonDir: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--git-common-dir"])
        return await normalizePath(resolveGitOutputPath(cwd, result.stdout.trim()))
      },
      resolveGitPath: async (cwd, gitPath) => {
        const result = await run(cwd, ["rev-parse", "--git-path", gitPath])
        return resolveGitOutputPath(cwd, result.stdout.trim())
      },
      isBareRepository: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--is-bare-repository"], {
          allowFailure: true,
        })
        return result.status === 0 && result.stdout.trim() === "true"
      },
    },
    refs: {
      resolve: async (cwd, refName) => {
        const result = await run(cwd, ["rev-parse", "--verify", "-q", refName], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
      exists: async (cwd, refName) => {
        const result = await run(cwd, ["rev-parse", "--verify", "--quiet", refName], {
          allowFailure: true,
        })
        return result.status === 0
      },
      update: async (cwd, refName, oid) => {
        await run(cwd, ["update-ref", refName, oid])
      },
      delete: async (cwd, refName) => {
        await run(cwd, ["update-ref", "-d", refName], {
          allowFailure: true,
        })
      },
      getCurrentBranch: async (cwd) => {
        const result = await run(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
      branchExists: async (cwd, branch) => {
        const result = await run(cwd, ["show-ref", "--verify", "--quiet", `refs/heads/${branch}`], {
          allowFailure: true,
        })
        return result.status === 0
      },
      getBranchHead: async (cwd, branch) => {
        const result = await run(cwd, ["rev-parse", "--verify", `refs/heads/${branch}`], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
    },
    history: {
      resolveHead: async (cwd) => {
        const result = await run(cwd, ["rev-parse", "--verify", "HEAD"], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
      isAncestor: async (cwd, ancestor, descendant) => {
        const result = await run(cwd, ["merge-base", "--is-ancestor", ancestor, descendant], {
          allowFailure: true,
        })
        return result.status === 0
      },
      getMergeBase: async (cwd, left, right) => {
        const result = await run(cwd, ["merge-base", left, right], {
          allowFailure: true,
        })
        return result.status === 0 ? result.stdout.trim() || null : null
      },
    },
    status: {
      getWorkingTreeStatus: async (cwd) => {
        const result = await run(cwd, ["status", "--porcelain=v1", "--untracked-files=all"])
        const entries = result.stdout
          .split("\n")
          .map((entry) => entry.trimEnd())
          .filter(Boolean)
        return {
          clean: entries.length === 0,
          entries,
        }
      },
      isWorktreeClean: async (cwd) => {
        const status = await createCliGitHost().status.getWorkingTreeStatus(cwd)
        return status.clean
      },
    },
    worktrees: {
      list: async (cwd) => {
        const result = await run(cwd, ["worktree", "list", "--porcelain"])
        return await parseGitWorktrees(result.stdout)
      },
    },
    stash: {
      list: async (cwd) => {
        const result = await run(cwd, ["stash", "list", "--format=%gd%x00%s"])
        return parseGitStashes(result.stdout)
      },
    },
  }
}
