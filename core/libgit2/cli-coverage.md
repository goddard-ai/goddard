# Git CLI Coverage

This document tracks Git behavior that is still implemented through `git` CLI subprocesses outside `@goddard-ai/libgit2`. Use it to decide which libgit2 bindings are worth adding next.

Last reviewed: 2026-06-25.

## Scope

- The subprocess seams are expected to live in package-local `git/` folders.
- `core/review-sync` has a package-local runner in `src/git/command.ts`, but several concrete command call sites still live outside `src/git/`. Those call sites are included here because they represent remaining CLI-owned behavior.
- Commands that already route through `@goddard-ai/libgit2.git` are not listed as CLI gaps, even when the libgit2 method currently throws an unsupported-operation error.

## Core Review Sync

Subprocess seam:

- `core/review-sync/src/git/command.ts`

Package-local adapter methods in `core/review-sync/src/git.ts`:

| Git capability | CLI command shape | Current purpose | Likely libgit2 surface |
| --- | --- | --- | --- |
| Repository root | `rev-parse --show-toplevel` | Resolve repository root for user paths. | Already covered by `git.repository.resolveRoot`. |
| Git dirs | `rev-parse --git-common-dir`, `rev-parse --git-dir` | Resolve common and per-worktree metadata dirs. | Already covered by `git.repository.resolveCommonDir` and `git.repository.resolveGitDir`. |
| Current branch | `symbolic-ref --quiet --short HEAD` | Read attached branch, returning null when detached. | Already covered by `git.refs.getCurrentBranch`. |
| Branch existence | `show-ref --verify --quiet refs/heads/<branch>` | Validate local branch availability. | Already covered by `git.refs.branchExists`. |
| Status clean check | `status --porcelain=v1 --untracked-files=all` | Block sync when user work is pending. | Implement `git.status.getWorkingTreeStatus` and `git.status.isWorktreeClean`. |
| Ref resolution | `rev-parse --verify -q <ref>` | Resolve hidden refs and HEAD-like names. | Mostly covered by `git.refs.resolve`; verify peeled commit behavior. |
| Ref mutation | `update-ref <ref> <oid>`, `update-ref -d <ref>` | Record and clean hidden review-sync refs. | Implement `git.refs.update` and `git.refs.delete`. |
| Worktree listing | `worktree list --porcelain` | Detect checked-out review branches and session worktrees. | Implement `git.worktrees.list`. |

Other review-sync call sites using the package runner:

| Git capability | CLI command shape | Current purpose | Likely libgit2 surface |
| --- | --- | --- | --- |
| Ancestry and merge base | `merge-base --is-ancestor`, `merge-base <left> <right>` | Review history checks. | Already covered by `git.history.isAncestor` and `git.history.getMergeBase`; call sites can move to `src/git/` wrappers. |
| Checkout and branch mutation | `checkout`, `checkout --detach`, `checkout -B`, `branch`, `branch -D` | Move review and agent worktrees between review/session branches. | Add checkout, branch create/reset/delete APIs only if review-sync mutation flows move native. |
| Reset and clean | `reset --hard`, `reset --mixed`, `clean -fd` | Restore worktrees and discard generated review state. | Add reset and clean APIs if native mutation coverage is required. |
| Patch application | `apply --check --binary`, `apply --binary` | Validate and apply human review patches. | Bind libgit2 apply APIs. |
| Index and tree writes | `read-tree`, `add -A`, `write-tree`, `commit-tree` | Build synthetic snapshots and commits. | Bind index, tree, and commit creation APIs. |
| Binary diffs | `diff --binary ...` | Compute patches for review-sync transfer. | Native diff support must preserve binary patch format or keep CLI for patch serialization. |
| Rebase state probe | `rebase --show-current-patch` | Distinguish stale `REBASE_HEAD` from active rebase. | Likely inspect rebase metadata directly rather than binding a rebase operation. |

## Core Sprint Branch

Subprocess seam:

- `core/sprint-branch/src/git/command.ts`

Current CLI-owned behavior:

| Git capability | CLI command shape | Current purpose | Likely libgit2 surface |
| --- | --- | --- | --- |
| Generic command execution | package-local `runGit(cwd, args)` | Sprint mutation flows still call arbitrary CLI commands outside the read wrappers. | Continue moving concrete commands into `src/git/` modules before adding native bindings. |
| Git-private path resolution | currently calls `git.repository.resolveGitPath` | Used for `.git/info/exclude` and operation markers. | Implement `git.repository.resolveGitPath`. |
| Status | currently calls `git.status.getWorkingTreeStatus` | Detect worktree dirtiness. | Implement `git.status.getWorkingTreeStatus`. |
| Stash listing | currently calls `git.stash.list` | Check recorded sprint stashes. | Implement `git.stash.list`. |

The read-only refs/history/repository calls in `src/git/refs.ts` and parts of `src/git/repository.ts` already route through `@goddard-ai/libgit2.git`.

## Pull Request Feature

Subprocess seam:

- `features/pull-request/src/daemon/git/command.ts`

Current CLI-owned behavior:

| Git capability | CLI command shape | Current purpose | Likely libgit2 surface |
| --- | --- | --- | --- |
| Remote HEAD symbolic ref | `symbolic-ref refs/remotes/origin/<ref>` | Infer the origin default branch. | Add remote-tracking symbolic ref lookup or generic symbolic ref read. |
| Remote URL config | `config --get remote.origin.url` | Infer GitHub repository owner/name. | Add config read support for repository config. |

Current branch inference already uses `git.refs.getCurrentBranch`.

## Session Feature

Subprocess seam:

- `features/session/src/daemon/git/command.ts`

Current CLI-owned behavior:

| Git capability | CLI command shape | Current purpose | Likely libgit2 surface |
| --- | --- | --- | --- |
| Checkout | `checkout <branch>`, `checkout -B ...` | Move session worktrees onto target branches. | Add checkout APIs if worktree mutation becomes native. |
| Diff serialization | `diff --no-ext-diff --binary --full-index`, `diff --no-index ...` | Build tracked, untracked, and initial workspace patches. | Native diff support must preserve binary/full-index patch output. |
| HEAD existence | `rev-parse --verify --quiet HEAD` | Decide whether a workspace has a commit baseline. | Already covered by `git.history.resolveHead`; call site can move. |
| File listing | `ls-files --others --exclude-standard`, `ls-files --cached --others --exclude-standard`, ignored variants | List untracked, cached, ignored, and excluded paths. | Add index/status path listing with ignore support. |
| Commit count | `rev-list --count <base>..HEAD` | Count unmerged worktree commits. | Add revwalk count support. |
| Ignore checks | `check-ignore -q`, `check-ignore --stdin -z` | Filter ignored paths and ignored directories. | Add ignore/pathspec matching support. |
| Local branch listing | `for-each-ref --format=... refs/heads` | List branches and identify the current one. | Add refs iteration for local branches. |
| Worktree add/remove/list | `worktree add --detach`, `worktree remove --force`, `worktree list` | Create, remove, and locate daemon worktrees. | Add worktree create/remove/list support. |
| Fetch PR head | `fetch origin pull/<n>/head:<branch>` | Materialize pull request refs in session worktrees. | Add remote fetch/refspec support only if PR worktree setup moves native. |

Several session modules also call `@goddard-ai/libgit2.git` directly for repository root, git dirs, branch existence, branch reads, status, and HEAD reads. The unsupported native gaps currently visible there are status, bare repository checks, and any other methods still throwing from `core/libgit2`.

## Suggested Binding Order

1. `git.status.isWorktreeClean` and `git.status.getWorkingTreeStatus`.
2. `git.repository.resolveGitPath`.
3. `git.refs.update` and `git.refs.delete`.
4. `git.worktrees.list`.
5. `git.stash.list`.
6. Config and symbolic-ref reads needed by pull-request defaults.
7. Ignore/path listing and revwalk count needed by session.
8. Mutation-heavy flows: checkout, branch create/delete, reset, clean, apply, index/tree/commit creation, fetch, and worktree add/remove.
