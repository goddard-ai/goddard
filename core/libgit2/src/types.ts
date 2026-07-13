export type WorktreeInfo = {
  path: string
  branch: string | null
}

export type WorkingTreeStatus = {
  clean: boolean
  entries: string[]
}

export type GitPathEntry = {
  path: string
  isDirectory: boolean
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
  readSymbolic: (cwd: string, refName: string) => Promise<string | null>
  listLocalBranches: (cwd: string) => Promise<string[]>
}

export type GitHistoryApi = {
  resolveHead: (cwd: string) => Promise<string | null>
  isAncestor: (cwd: string, ancestor: string, descendant: string) => Promise<boolean>
  getMergeBase: (cwd: string, left: string, right: string) => Promise<string | null>
  countCommits: (cwd: string, range: { from: string; to?: string }) => Promise<number>
}

export type GitStatusApi = {
  getWorkingTreeStatus: (cwd: string) => Promise<WorkingTreeStatus>
  isWorktreeClean: (cwd: string) => Promise<boolean>
  listUntracked: (
    cwd: string,
    options?: { collapseDirectories?: boolean },
  ) => Promise<GitPathEntry[]>
}

export type GitConfigApi = {
  get: (cwd: string, name: string) => Promise<string | null>
}

export type GitIgnoreApi = {
  isIgnored: (cwd: string, path: string) => Promise<boolean>
  filterIgnored: (cwd: string, paths: string[]) => Promise<Set<string>>
}

export type GitIndexApi = {
  listPaths: (cwd: string) => Promise<string[]>
}

export type GitWorktreeApi = {
  list: (cwd: string) => Promise<WorktreeInfo[]>
}

export type GitStashApi = {
  list: (cwd: string) => Promise<Map<string, string>>
}

export type GitApi = {
  repository: GitRepositoryApi
  refs: GitRefsApi
  history: GitHistoryApi
  status: GitStatusApi
  config: GitConfigApi
  ignore: GitIgnoreApi
  index: GitIndexApi
  worktrees: GitWorktreeApi
  stash: GitStashApi
}
