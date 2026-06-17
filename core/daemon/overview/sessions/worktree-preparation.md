# Worktree Preparation

- **Core idea**
  - Fresh isolated session worktrees can be prepared before the agent starts.
  - Preparation reduces repeated setup work without making copied artifacts the source of truth.

- **Eligible worktrees**
  - Preparation applies to newly created isolated worktrees managed by the daemon's built-in worktree path.
  - Reused worktrees skip fresh preparation.
  - Custom worktree providers own their own setup behavior.

- **Artifact seeding**
  - The daemon may reuse configured untracked artifacts from the source checkout.
  - Seeding is allowed only when the fresh worktree starts from the same commit as the source checkout.
  - Seeding is best-effort and should not copy arbitrary untracked files.

- **Bootstrap**
  - The daemon may run a package-manager bootstrap when repository intent or unambiguous inference supports it.
  - If no package manager can be resolved, bootstrap can be skipped.
  - If a package manager is resolved and the bootstrap fails, session launch fails instead of starting from a partially prepared checkout.

- **Trust boundary**
  - Repository-local configuration can declare non-executable preparation intent.
  - Repository-local arbitrary shell hooks are not part of worktree preparation.
  - User-scoped executable worktree extensions remain separate from repository-local preparation intent.
