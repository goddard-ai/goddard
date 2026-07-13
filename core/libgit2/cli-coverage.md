# Git CLI Coverage

This document tracks production Git behavior that Goddard packages still implement through `git` subprocesses instead of `@goddard-ai/libgit2`. Tests, smoke scripts, repository hooks, and development tooling are outside this inventory.

Last reviewed: 2026-07-12.

## Native Coverage

The exported `git` namespace now implements these operations with libgit2:

- repository root, Git directory, common directory, Git-private paths, and bare-repository checks
- status and clean checks, including non-ignored untracked files
- direct ref resolution, creation, update, deletion, symbolic reads, and local branch listing
- HEAD resolution, ancestry, merge base, and commit-range counting
- worktree listing
- repository config reads, ignore checks, and index path listing

Review-sync, session, and pull-request callers use these native methods where their required behavior matches libgit2. The pull-request feature no longer has a Git CLI subprocess seam. The desktop runtime and workforce command still have native-compatible read operations to migrate.

## Core Review Sync

Subprocess seam: `core/review-sync/src/git/command.ts`.

| CLI-owned capability | Command shapes | Why it remains CLI-backed |
| --- | --- | --- |
| Checkout and branch mutation | `checkout`, `checkout --detach`, `branch`, `branch -D` | Review and recovery flows depend on Git porcelain's validation and worktree safety behavior. Convert only as an end-to-end workflow with equivalent failure tests. |
| Reset and clean | `reset --hard`, `reset --mixed`, `clean -fd` | These destructive multi-surface mutations need explicit index, worktree, and untracked-file parity before migration. |
| Patch validation and application | `apply --check --binary`, `apply --binary` | Human review patches require compatibility with Git's binary patch format and preflight semantics. |
| Synthetic snapshots | `read-tree`, `add -A`, `write-tree`, `commit-tree` | libgit2 can build indexes, trees, and commits, but review-sync's exact snapshot behavior should move as one separately tested unit. |
| Patch serialization | `diff --binary` | The serialized patch is an interchange format, so byte-level compatibility matters more than avoiding a subprocess. |
| Rebase-state probe | `rebase --show-current-patch` | This currently distinguishes stale metadata from an active rebase. It is a candidate for repository-state inspection without performing a native rebase. |

Several concrete review-sync command calls still live outside `src/git/`. Moving those command-specific wrappers under `src/git/` remains package-local organization work and does not require expanding `@goddard-ai/libgit2`.

## Session Feature

Subprocess seam: `features/session/src/daemon/git/command.ts`.

| CLI-owned capability | Command shapes | Why it remains CLI-backed |
| --- | --- | --- |
| Checkout and branch reset | `checkout`, `checkout -B` | Session setup combines branch creation/reset and checkout semantics; migrate as one workflow with failure coverage. |
| Worktree mutation | `worktree add --detach`, `worktree remove --force` | Native worktree mutation is feasible but needs cleanup, lock, and partial-failure tests. Worktree listing is already native. |
| Pull request fetch | `fetch origin pull/<n>/head:<branch>` | Keep network operations on CLI until native credential, proxy, certificate, and progress handling are designed. |
| Binary diff serialization | `diff --binary --full-index`, `diff --no-index` | Session patches must remain compatible with Git's patch parser, including binary files and added-file formatting. |
| Empty-directory discovery | `ls-files --others --exclude-standard --directory` | libgit2 status does not report empty untracked directories; session bootstrap intentionally copies an empty `node_modules`. |
| Custom include matching | `ls-files --others --ignored --exclude-from=<file>` | `.worktreeinclude` relies on Git's exclude-file matching and directory collapsing. Keep it on CLI unless a native implementation proves equivalent pattern semantics. |

## Desktop Runtime

Subprocess seam: `app/src/bun/project-git-status.ts`.

| CLI-owned capability | Command shapes | Migration assessment |
| --- | --- | --- |
| Checkout identity | `rev-parse --short HEAD`, `rev-parse --show-toplevel` | The native API already resolves `HEAD` and repository roots. Shortening an object id is presentation logic, so these reads can migrate without new bindings. |
| Checkout status | `status --porcelain=v1 -b` | Native status and current-branch reads already cover changes, untracked files, and branch identity. Ahead/behind reporting needs an explicit upstream-resolution API before the complete result can migrate. |
| Linked worktree discovery | `worktree list --porcelain` | Native worktree listing already covers this behavior and should replace the subprocess call. |

## Workforce Command

Subprocess seam: `workforce/src/main.ts`.

| CLI-owned capability | Command shapes | Migration assessment |
| --- | --- | --- |
| Repository root discovery | `rev-parse --show-toplevel` | The native repository API already supports this operation. This call can migrate directly once workforce depends on `@goddard-ai/libgit2`. |

## Recommended Boundary

Keep these operations CLI-backed unless a future task explicitly accepts their parity and recovery risk:

- binary patch serialization, validation, and application
- fetch and other network operations
- merge and rebase porcelain
- broad checkout, reset, clean, and branch-mutation workflows
- empty untracked directory and custom exclude-file enumeration

Good later native candidates are worktree add/remove, repository-state probes, and review-sync snapshot creation. Each should be migrated as a complete workflow with real-repository tests rather than as unused bindings.

The desktop runtime's repository, status, and worktree reads and workforce's repository-root read are smaller migration candidates because the native API already implements most or all of their behavior.
