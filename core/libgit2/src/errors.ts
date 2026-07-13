/** Base error for native library loading and libgit2 operation failures. */
export class GitHostError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options)
    this.name = "GitHostError"
  }
}

/** Reports that libgit2 could not open a path as a Git repository. */
export class GitNotRepositoryError extends GitHostError {
  constructor(cwd: string) {
    super(`Not a Git worktree: ${cwd}`)
    this.name = "GitNotRepositoryError"
  }
}
