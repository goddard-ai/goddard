# libgit2 Package

The libgit2 package owns Goddard's direct `libgit2` integration. It exposes a lazy Git API backed by the packaged native library and keeps command-line Git outside this package.

## Package Entrypoints

- `@goddard-ai/libgit2` is the primary package entrypoint for runtime Git access.
- `@goddard-ai/libgit2/testing` provides `createFakeGitApi` for tests that need a deterministic in-memory implementation of the Git API contract.

## Runtime Access

- `git` is the shared libgit2-backed Git API namespace.
- Accessing a direct namespace such as `git.repository` or `git.refs` lazily loads the libgit2 runtime once and reuses it.
- `validateLibgit2Runtime(options)` verifies that a libgit2 runtime can be loaded from the supplied candidates.
- `resetGitForTests()` clears cached libgit2 and Git API state between tests.

## Git API Contract

- `GitApi` groups the supported Git capability areas:
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
- `GitApi` describes the shared `git` namespace shape.
- `GitRepositoryApi`, `GitRefsApi`, `GitHistoryApi`, `GitStatusApi`, `GitWorktreeApi`, and `GitStashApi` describe each section of the Git API contract.

## Errors

- `GitHostError` is the base package error.
- `GitNotRepositoryError` reports that a path is not a Git worktree.

## Path Handling

- `normalizePath(path)` resolves a path through the filesystem and returns its canonical real path.
