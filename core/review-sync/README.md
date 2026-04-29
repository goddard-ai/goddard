# `@goddard-ai/review-sync`

`@goddard-ai/review-sync` is a small Git-only operator for reviewing an agent-owned
branch from a separate local worktree.

The package derives a disposable review branch by appending `--review` to the full
agent branch name. Each sync captures the review worktree against the last rendered
snapshot, tries to apply the resulting human patch to the agent worktree, saves the
patch as accepted or rejected, then refreshes the review branch from the agent
snapshot.

This package is separate from daemon worktree sync. The existing daemon feature
continues to own mounted live synchronization between a primary checkout and a
daemon session worktree.

## CLI

```bash
review-sync start --review-worktree <path>
review-sync sync
review-sync status
review-sync status --json
review-sync pause
review-sync resume
```

`start` runs from the agent worktree. The other commands may run from either the
agent worktree or the review worktree.

## API

```ts
import { runReviewSync } from "@goddard-ai/review-sync"

await runReviewSync(["sync"])
```

The public JavaScript entrypoint is `runReviewSync`; it uses the current process
working directory and returns a structured result without writing CLI output.
Internal Git helpers, state readers, lock handling, and snapshot builders are
intentionally not exported.

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
