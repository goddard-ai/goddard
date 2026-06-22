# Git Host Package

The Git host package provides Goddard's shared Git access boundary so daemon capabilities can use one consistent contract for repository discovery, refs, history, status, worktrees, and stashes.

## Package Entrypoints

- `@goddard-ai/git` is the primary package entrypoint for runtime Git access.
- `@goddard-ai/git/testing` provides `createFakeGitHost` for tests that need a deterministic in-memory implementation of the host contract.

## Host Creation

- `createGitHost(options)` creates a `GitHost` using the requested mode or the environment-driven default.
- `createCliGitHost()` creates a host backed by the Git command-line tool.
- `createLibgit2GitHost(fallback, options)` creates a hybrid host that uses libgit2 for supported operations and delegates unsupported operations to the provided fallback host.
- `validateLibgit2Runtime(options)` verifies that a libgit2 runtime can be loaded from the supplied candidates.
- `resetGitHostForTests()` clears cached libgit2 state between tests.

## Host Modes

- `auto` attempts the hybrid libgit2 host and falls back to CLI Git if libgit2 cannot be loaded.
- `cli` uses the Git command-line host directly.
- `libgit2` requires libgit2 to load successfully.
- `resolveGitHostMode(env)` chooses the mode from environment values:
  - `GODDARD_GIT_HOST=cli` selects CLI Git.
  - `GODDARD_GIT_HOST=libgit2` selects libgit2.
  - `GODDARD_GIT_LIBGIT2_PATH` selects libgit2 when no explicit host mode is set.
  - no matching value selects `auto`.

## Host Contract

- `GitHost` groups the supported Git capability areas:
  - `repository` resolves repository roots, Git directories, common directories, Git paths, and bare repository status.
  - `refs` resolves refs, checks ref and branch existence, updates refs, deletes refs, reads the current branch, and reads branch heads.
  - `history` resolves `HEAD`, checks ancestor relationships, and finds merge bases.
  - `status` reads working tree status and clean or dirty state.
  - `worktrees` lists known worktrees with paths and branch names.
  - `stash` lists stashes by ref and message.

## Public Data Types

- `WorktreeInfo` describes a worktree path and its branch name when one is available.
- `WorkingTreeStatus` describes whether a worktree is clean and includes raw porcelain status entries.
- `GitHostOptions` configures host mode and libgit2 path candidates.
- `GitHostMode` is one of `auto`, `cli`, or `libgit2`.
- `GitRepositoryApi`, `GitRefsApi`, `GitHistoryApi`, `GitStatusApi`, `GitWorktreeApi`, and `GitStashApi` describe each section of the host contract.

## Errors

- `GitHostError` is the base package error.
- `GitNotRepositoryError` reports that a path is not a Git worktree.
- `GitCommandError` reports failed host operations backed by CLI Git and keeps the command arguments, cwd, stdout, stderr, and status available to callers.

## Path Handling

- `normalizePath(path)` resolves a path through the filesystem and returns its canonical real path.
