/** Shared Git access boundary for daemon-owned repository behavior. */
export { GitHostError, GitNotRepositoryError } from "./errors.ts"
export { git, resetGitForTests, validateLibgit2Runtime } from "./libgit2/host.ts"
export { normalizePath } from "./paths.ts"
export type {
  GitApi,
  GitConfigApi,
  GitHistoryApi,
  GitIgnoreApi,
  GitIndexApi,
  GitPathEntry,
  GitRefsApi,
  GitRepositoryApi,
  GitStatusApi,
  GitWorktreeApi,
  WorkingTreeStatus,
  WorktreeInfo,
} from "./types.ts"
