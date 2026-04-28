import { execFile } from "node:child_process"
import { promisify } from "node:util"

const execFileAsync = promisify(execFile)

/** Error wrapper that preserves the failed Git argv for diagnostics. */
export class GitCommandError extends Error {
  args: string[]
  stdout: string
  stderr: string
  code: number | null

  constructor(args: string[], error: unknown) {
    const record = error as {
      message?: string
      stdout?: string | Buffer
      stderr?: string | Buffer
      code?: number
    }
    super(record.message ?? `git ${args.join(" ")} failed`)
    this.name = "GitCommandError"
    this.args = args
    this.stdout = String(record.stdout ?? "")
    this.stderr = String(record.stderr ?? "")
    this.code = typeof record.code === "number" ? record.code : null
  }
}

/** Runs one Git command with read-only lock avoidance by default. */
export async function runGit(cwd: string, args: string[]) {
  try {
    const result = await execFileAsync("git", args, {
      cwd,
      env: {
        ...process.env,
        GIT_OPTIONAL_LOCKS: "0",
      },
      maxBuffer: 20 * 1024 * 1024,
    })
    return String(result.stdout)
  } catch (error) {
    throw new GitCommandError(args, error)
  }
}

/** Returns true when the Git command exits successfully. */
export async function gitSucceeds(cwd: string, args: string[]) {
  try {
    await runGit(cwd, args)
    return true
  } catch (error) {
    if (error instanceof GitCommandError) {
      return false
    }
    throw error
  }
}

/** Resolves the root directory of the Git repository containing the start directory. */
export async function resolveRepositoryRoot(startDir: string) {
  return (await runGit(startDir, ["rev-parse", "--show-toplevel"])).trim()
}

/** Resolves the current branch, returning null for detached HEAD. */
export async function getCurrentBranch(rootDir: string) {
  try {
    return (await runGit(rootDir, ["symbolic-ref", "--quiet", "--short", "HEAD"])).trim()
  } catch (error) {
    if (error instanceof GitCommandError) {
      return null
    }
    throw error
  }
}

/** Checks whether a local branch exists. */
export async function branchExists(rootDir: string, branch: string) {
  return gitSucceeds(rootDir, ["rev-parse", "--verify", "--quiet", `refs/heads/${branch}`])
}

/** Returns the current commit for a local branch when it exists. */
export async function getBranchHead(rootDir: string, branch: string) {
  try {
    return (await runGit(rootDir, ["rev-parse", "--verify", `refs/heads/${branch}`])).trim()
  } catch (error) {
    if (error instanceof GitCommandError) {
      return null
    }
    throw error
  }
}

/** Checks whether the possible ancestor commit is contained by the descendant ref. */
export async function isAncestor(rootDir: string, ancestor: string, descendant: string) {
  return gitSucceeds(rootDir, ["merge-base", "--is-ancestor", ancestor, descendant])
}

/** Reads porcelain working tree status without mutating the repository. */
export async function getWorkingTreeStatus(rootDir: string) {
  const entries = (await runGit(rootDir, ["status", "--porcelain=v1"]))
    .split("\n")
    .map((entry) => entry.trimEnd())
    .filter(Boolean)

  return {
    clean: entries.length === 0,
    entries,
  }
}

/** Reads stash refs and messages so recorded sprint stashes can be checked. */
export async function getStashRefs(rootDir: string) {
  const stdout = await runGit(rootDir, ["stash", "list", "--format=%gd%x00%s"])
  return new Map(
    stdout
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        const [ref, message = ""] = line.split("\0")
        return [ref, message] as const
      }),
  )
}
