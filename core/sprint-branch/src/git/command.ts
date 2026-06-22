import { spawn } from "node:child_process"
import { createGitHost, type GitHost } from "@goddard-ai/libgit2"

let sharedGitHost: GitHost | undefined

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
  const result = await runGitCommand(cwd, args, {
    env: {
      GIT_OPTIONAL_LOCKS: "0",
    },
  })
  if (result.status !== 0) {
    throw new GitCommandError(args, {
      message: `git ${args.join(" ")} failed`,
      stdout: result.stdout,
      stderr: result.stderr,
      code: result.status,
    })
  }

  return result.stdout
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

export function getSharedGitHost() {
  sharedGitHost ??= createGitHost()
  return sharedGitHost
}

async function runGitCommand(
  cwd: string,
  args: string[],
  options: { env?: Record<string, string | undefined> } = {},
) {
  return await new Promise<{ status: number; stdout: string; stderr: string }>(
    (resolvePromise, rejectPromise) => {
      const child = spawn("git", args, {
        cwd,
        env: {
          ...process.env,
          ...options.env,
        },
        stdio: ["ignore", "pipe", "pipe"],
      })

      let stdout = ""
      let stderr = ""
      child.stdout.setEncoding("utf8")
      child.stderr.setEncoding("utf8")
      child.stdout.on("data", (chunk: string) => {
        stdout += chunk
      })
      child.stderr.on("data", (chunk: string) => {
        stderr += chunk
      })
      child.on("error", rejectPromise)
      child.on("close", (status) => {
        resolvePromise({
          status: status ?? 1,
          stdout,
          stderr,
        })
      })
    },
  )
}
