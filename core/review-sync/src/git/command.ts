import { spawn } from "node:child_process"

export type GitCommandResult = {
  status: number
  stdout: string
  stderr: string
}

export type GitRunOptions = {
  allowFailure?: boolean
  stdin?: string | "ignore"
  env?: Record<string, string | undefined>
}

class GitCommandError extends Error {
  args: string[]
  cwd: string
  stdout: string
  stderr: string
  status: number

  constructor(cwd: string, args: string[], result: GitCommandResult) {
    super(
      `git ${args.join(" ")} failed in ${cwd}: ${
        result.stderr.trim() || result.stdout.trim() || "unknown Git error"
      }`,
    )
    this.name = "GitCommandError"
    this.cwd = cwd
    this.args = args
    this.stdout = result.stdout
    this.stderr = result.stderr
    this.status = result.status
  }
}

export async function runReviewSyncGitCommand(
  cwd: string,
  args: string[],
  options: GitRunOptions = {},
) {
  const result = await runGitCommand(cwd, args, options)
  if (result.status !== 0 && options.allowFailure !== true) {
    throw new GitCommandError(cwd, args, result)
  }

  return result
}

async function runGitCommand(cwd: string, args: string[], options: GitRunOptions = {}) {
  return await new Promise<GitCommandResult>((resolvePromise, rejectPromise) => {
    const child = spawn("git", args, {
      cwd,
      env: { ...process.env, ...options.env },
      stdio: ["pipe", "pipe", "pipe"],
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
      resolvePromise({ status: status ?? 1, stdout, stderr })
    })

    if (options.stdin !== undefined && options.stdin !== "ignore") {
      child.stdin.end(options.stdin)
    } else {
      child.stdin.end()
    }
  })
}
