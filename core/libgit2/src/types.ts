export type WorktreeInfo = {
  path: string
  branch: string | null
}

export type WorkingTreeStatus = {
  clean: boolean
  entries: string[]
}

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

export type GitHostMode = "auto" | "cli" | "libgit2"

export type GitHostOptions = {
  mode?: GitHostMode
  libgit2PathCandidates?: string[]
}

export type GitRepositoryApi = {
  resolveRoot: (cwd: string) => Promise<string>
  resolveGitDir: (cwd: string) => Promise<string>
  resolveCommonDir: (cwd: string) => Promise<string>
  resolveGitPath: (cwd: string, gitPath: string) => Promise<string>
  isBareRepository: (cwd: string) => Promise<boolean>
}

export type GitRefsApi = {
  resolve: (cwd: string, refName: string) => Promise<string | null>
  exists: (cwd: string, refName: string) => Promise<boolean>
  update: (cwd: string, refName: string, oid: string) => Promise<void>
  delete: (cwd: string, refName: string) => Promise<void>
  getCurrentBranch: (cwd: string) => Promise<string | null>
  branchExists: (cwd: string, branch: string) => Promise<boolean>
  getBranchHead: (cwd: string, branch: string) => Promise<string | null>
}

export type GitHistoryApi = {
  resolveHead: (cwd: string) => Promise<string | null>
  isAncestor: (cwd: string, ancestor: string, descendant: string) => Promise<boolean>
  getMergeBase: (cwd: string, left: string, right: string) => Promise<string | null>
}

export type GitStatusApi = {
  getWorkingTreeStatus: (cwd: string) => Promise<WorkingTreeStatus>
  isWorktreeClean: (cwd: string) => Promise<boolean>
}

export type GitWorktreeApi = {
  list: (cwd: string) => Promise<WorktreeInfo[]>
}

export type GitStashApi = {
  list: (cwd: string) => Promise<Map<string, string>>
}

export type GitHost = {
  repository: GitRepositoryApi
  refs: GitRefsApi
  history: GitHistoryApi
  status: GitStatusApi
  worktrees: GitWorktreeApi
  stash: GitStashApi
}
