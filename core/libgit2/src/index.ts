/** Shared Git access boundary for daemon-owned repository behavior. */
export { GitHostError, GitNotRepositoryError } from "./errors.ts"
export { createGitHost, resetGitHostForTests } from "./factory.ts"
export { createLibgit2GitHost, validateLibgit2Runtime } from "./libgit2/host.ts"
export { normalizePath } from "./paths.ts"
export type {
  GitHistoryApi,
  GitHost,
  GitHostOptions,
  GitRefsApi,
  GitRepositoryApi,
  GitStashApi,
  GitStatusApi,
  GitWorktreeApi,
  WorkingTreeStatus,
  WorktreeInfo,
} from "./types.ts"
