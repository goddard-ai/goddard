/** One main or linked worktree, with a short local branch name when attached. */
export type WorktreeInfo = {
  path: string
  branch: string | null
}

/** A worktree cleanliness result with Git porcelain-style `XY path` entries. */
export type WorkingTreeStatus = {
  clean: boolean
  entries: string[]
}

/** One non-ignored untracked path relative to the repository worktree. */
export type GitPathEntry = {
  path: string
  isDirectory: boolean
}

export type GitRepositoryApi = {
  /** Resolves the canonical worktree root containing `cwd`; bare repositories throw. */
  resolveRoot: (cwd: string) => Promise<string>
  /** Resolves the canonical per-worktree Git metadata directory. */
  resolveGitDir: (cwd: string) => Promise<string>
  /** Resolves the canonical shared Git metadata directory. */
  resolveCommonDir: (cwd: string) => Promise<string>
  /** Reports whether `cwd` belongs to a bare repository. */
  isBareRepository: (cwd: string) => Promise<boolean>
}

export type GitRefsApi = {
  /** Resolves a revision expression to its full object ID, or returns null when unresolved. */
  resolve: (cwd: string, refName: string) => Promise<string | null>
  /** Creates or force-updates a direct, fully qualified ref to a full object ID. */
  update: (cwd: string, refName: string, oid: string) => Promise<void>
  /** Deletes a fully qualified ref; an absent ref is already considered deleted. */
  delete: (cwd: string, refName: string) => Promise<void>
  /** Returns the short local branch name, or null for detached or unborn HEAD. */
  getCurrentBranch: (cwd: string) => Promise<string | null>
  /** Reports whether `refs/heads/<branch>` resolves. */
  branchExists: (cwd: string, branch: string) => Promise<boolean>
  /** Returns a symbolic ref's fully qualified target, or null for missing or direct refs. */
  readSymbolic: (cwd: string, refName: string) => Promise<string | null>
  /** Lists sorted short names from `refs/heads/`. */
  listLocalBranches: (cwd: string) => Promise<string[]>
}

export type GitHistoryApi = {
  /** Resolves HEAD to its full object ID, or returns null for unborn HEAD. */
  resolveHead: (cwd: string) => Promise<string | null>
  /** Reports whether both revisions resolve and `ancestor` is reachable from `descendant`. */
  isAncestor: (cwd: string, ancestor: string, descendant: string) => Promise<boolean>
  /** Returns the merge-base object ID, or null when either revision or a merge base is absent. */
  getMergeBase: (cwd: string, left: string, right: string) => Promise<string | null>
  /** Counts commits reachable from `to` or HEAD, excluding commits reachable from `from`. */
  countCommits: (cwd: string, range: { from: string; to?: string }) => Promise<number>
}

export type GitStatusApi = {
  /** Reads staged, unstaged, conflicted, and non-ignored untracked status entries. */
  getWorkingTreeStatus: (cwd: string) => Promise<WorkingTreeStatus>
  /** Reports whether no staged, unstaged, conflicted, or untracked changes exist. */
  isWorktreeClean: (cwd: string) => Promise<boolean>
  /** Lists non-ignored untracked paths, optionally collapsing non-empty directories. */
  listUntracked: (
    cwd: string,
    options?: { collapseDirectories?: boolean },
  ) => Promise<GitPathEntry[]>
}

export type GitConfigApi = {
  /** Reads the repository's resolved config snapshot, or returns null when the key is absent. */
  get: (cwd: string, name: string) => Promise<string | null>
}

export type GitIgnoreApi = {
  /** Applies the repository's standard ignore rules to one worktree-relative path. */
  isIgnored: (cwd: string, path: string) => Promise<boolean>
  /** Returns the subset of worktree-relative input paths ignored by standard rules. */
  filterIgnored: (cwd: string, paths: string[]) => Promise<Set<string>>
}

export type GitIndexApi = {
  /** Lists unique repository-relative paths currently present in the index. */
  listPaths: (cwd: string) => Promise<string[]>
}

export type GitWorktreeApi = {
  /** Lists the main worktree followed by linked worktrees known to the repository. */
  list: (cwd: string) => Promise<WorktreeInfo[]>
}

/** Shared lazy namespaces for Goddard's supported native Git operations. */
export type GitApi = {
  repository: GitRepositoryApi
  refs: GitRefsApi
  history: GitHistoryApi
  status: GitStatusApi
  config: GitConfigApi
  ignore: GitIgnoreApi
  index: GitIndexApi
  worktrees: GitWorktreeApi
}
