/** Shared Git access boundary for daemon-owned repository behavior. */
export { createCliGitHost } from "./cli/host.ts"
export { GitCommandError, GitHostError, GitNotRepositoryError } from "./errors.ts"
export { createGitHost, resetGitHostForTests, resolveGitHostMode } from "./factory.ts"
export { createLibgit2GitHost, validateLibgit2Runtime } from "./libgit2/host.ts"
export { normalizePath } from "./paths.ts"
export type {
  GitCommandResult,
  GitHistoryApi,
  GitHost,
  GitHostMode,
  GitHostOptions,
  GitRefsApi,
  GitRepositoryApi,
  GitRunOptions,
  GitStashApi,
  GitStatusApi,
  GitWorktreeApi,
  WorkingTreeStatus,
  WorktreeInfo,
} from "./types.ts"
