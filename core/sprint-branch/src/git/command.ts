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
