# `@goddard-ai/review-sync`

`@goddard-ai/review-sync` is a small Git-only operator for reviewing an agent-owned
branch from a separate local worktree.

The package derives a disposable review branch by prepending `review-sync/` to the
full agent branch name. Each sync captures the review worktree against the last rendered
snapshot, tries to apply the resulting human patch to the agent worktree, saves the
patch as accepted or rejected, then refreshes the review branch from the agent
snapshot.

This package is separate from daemon worktree sync. The existing daemon feature
continues to own mounted live synchronization between a primary checkout and a
daemon session worktree.

## CLI

```bash
review-sync start <agent-branch>
review-sync start
review-sync sync
review-sync status
review-sync status --json
review-sync pause
review-sync resume
review-sync watch <agent-branch>
review-sync watch --verbose <agent-branch>
review-sync watch
```

`start` runs from the review worktree. It resolves `<agent-branch>` to the
matching branch already checked out in another worktree. When no branch is
provided in an interactive terminal, `start` opens a filterable prompt for one
of the eligible checked-out agent branches. The other commands may run from
either the agent worktree or the review worktree. `watch` monitors both worktrees
and their Git metadata, then runs `sync` when either worktree's branch, `HEAD`,
or snapshot tree changes.
Pass `--verbose` to `watch` to print session resolution, watched paths,
coalesced filesystem events, sync decisions, and cleanup steps while it runs.
Passing `<agent-branch>` to `watch` starts or reuses the review-sync session
before watching, so a separate `start` command is not required. If that branch
is not currently checked out in an agent worktree, `watch` still checks out the
derived review branch and resets it to the branch ref when the current worktree
is clean and the branch ref exists. It then waits for Git metadata to show an
agent checkout before starting the session.
Human commits or dirty edits made on that prepared review branch are preserved
and applied once an agent checkout becomes available.
When a saved session starts from another review-worktree branch, `watch` checks
out the review branch first using the same clean-worktree guard as `start`.
If the agent worktree temporarily checks out another branch while `watch` is
running, `watch` waits for the recorded agent branch to be checked out again
before retrying sync. While waiting, it still refreshes the review worktree from
the recorded agent branch ref when no unapplied human patch would be discarded.
If review-side edits block that refresh, `watch` prints a warning and leaves
those edits in place.
Once a session is active, `watch` pauses the review-sync session before exiting
and attempts to check out the review worktree branch that was active when
`watch` started. If cleanup cannot complete, the final command output explains
which step failed and why.
One-shot `start` and `sync` commands fail fast when the agent worktree is on a
different branch.

## API

```ts
import { startReviewSync, syncReviewSession } from "@goddard-ai/review-sync"

await startReviewSync({
  cwd: "/repo-review",
  agentBranch: "codex/my-agent-branch",
})

await syncReviewSession({ cwd: "/repo-agent" })
```

The package exports command-level functions for TypeScript callers:
`startReviewSync`, `syncReviewSession`, `statusReviewSession`,
`pauseReviewSession`, `resumeReviewSession`, and `watchReviewSession`.
`runReviewSync(argv)` remains available for argv-compatible wrappers and uses the
current process working directory. Internal Git helpers, state readers, lock
handling, and snapshot builders are intentionally not exported.

## Smoke Test

Run the happy-path smoke test manually with:

```bash
bun run --cwd core/review-sync smoke
```

The smoke test creates one temporary Git repository with agent and review
worktrees, starts review sync, then runs tracked-file, untracked-file, and
committed-change syncs from both the review worktree and the agent worktree. It
removes the temp repository on success.

## State

Durable state lives under the repository's Git common directory:

```text
<git-common-dir>/review-sync/sessions/<session-id>/
  state.json
  lock/
  events.ndjson
  patches/
    accepted/
    rejected/
    pending/
```

Snapshot commits are stored behind hidden refs under `refs/review-sync/`.
