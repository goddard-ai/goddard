import type { GitCommandResult } from "./types.ts"

export class GitHostError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options)
    this.name = "GitHostError"
  }
}

export class GitNotRepositoryError extends GitHostError {
  constructor(cwd: string) {
    super(`Not a Git worktree: ${cwd}`)
    this.name = "GitNotRepositoryError"
  }
}

export class GitCommandError extends GitHostError {
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
