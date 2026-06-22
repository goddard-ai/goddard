# libgit2 Package

The libgit2 package owns Goddard's direct `libgit2` integration. It loads the packaged native library, exposes the libgit2-backed host contract, and keeps command-line Git outside this package.

## Package Entrypoints

- `@goddard-ai/libgit2` is the primary package entrypoint for runtime Git access.
- `@goddard-ai/libgit2/testing` provides `createFakeGitHost` for tests that need a deterministic in-memory implementation of the host contract.

## Host Creation

- `createGitHost(options)` creates a libgit2-backed `GitHost`.
- `createLibgit2GitHost(options)` creates the same libgit2-backed host directly.
- `validateLibgit2Runtime(options)` verifies that a libgit2 runtime can be loaded from the supplied candidates.
- `resetGitHostForTests()` clears cached libgit2 state between tests.

## Host Contract

- `GitHost` groups the supported Git capability areas:
  - `repository` resolves repository roots, Git directories, common directories, Git paths, and bare repository status.
  - `refs` resolves refs, checks ref and branch existence, updates refs, deletes refs, reads the current branch, and reads branch heads.
  - `history` resolves `HEAD`, checks ancestor relationships, and finds merge bases.
  - `status` reads working tree status and clean or dirty state.
  - `worktrees` lists known worktrees with paths and branch names.
  - `stash` lists stashes by ref and message.
- Methods not yet implemented through libgit2 reject with `GitHostError`. Consumers that still need command-line behavior should own those wrappers in their package.

## Public Data Types

- `WorktreeInfo` describes a worktree path and its branch name when one is available.
- `WorkingTreeStatus` describes whether a worktree is clean and includes raw porcelain status entries.
- `GitHostOptions` configures libgit2 path candidates.
- `GitRepositoryApi`, `GitRefsApi`, `GitHistoryApi`, `GitStatusApi`, `GitWorktreeApi`, and `GitStashApi` describe each section of the host contract.

## Errors

- `GitHostError` is the base package error.
- `GitNotRepositoryError` reports that a path is not a Git worktree.

## Path Handling

- `normalizePath(path)` resolves a path through the filesystem and returns its canonical real path.
