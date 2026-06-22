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
