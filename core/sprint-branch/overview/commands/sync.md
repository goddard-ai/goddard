# `sprint-branch sync`

- **Question it answers**
  - How do I watch the active sprint review branch through the review-sync
    workflow?

- **Sprint selection**
  - Uses [standard sprint selection](../sprint-selection.md).

- **What it does**
  - Resolves the active sprint.
  - Validates sprint status.
  - Starts a `review-sync` watch session for the sprint's review branch.
  - If another sprint branch operation is active, waits for that operation to
    release the sprint lock before starting the initial review-sync watch.
  - Surfaces `review-sync` results while running.
  - Exits with the review-sync outcome.
  - Can be stopped from another shell by running `sprint-branch stop-sync` in
    the same working directory.
  - With `--replace`, first asks any existing `sync` process from the same
    working directory to stop, waits briefly for cleanup, then starts watching.

- **What it changes**
  - It delegates to the separate `review-sync` workflow.
  - `review-sync` keeps a disposable review branch aligned with the agent-owned
    branch while preserving human review edits.
  - It does not advance sprint task state by itself.

- **Guardrails**
  - If sprint status is invalid, `sync` reports sprint diagnostics instead of
    starting the watch.
  - Without `--replace`, refuses to start when another `sync` process is already
    registered for the same resolved working directory.
  - While watching, defers review-sync refreshes triggered during an active
    sprint branch operation and performs one refresh after the lock is released.
  - Stale sprint branch locks are removed instead of blocking sync forever.
  - The sprint review branch is the agent branch passed into `review-sync`.
  - The command exits with the `review-sync` outcome when the watch session
    starts.
  - Stop requests only apply to `sync` processes started from the same resolved
    working directory.

- **Why it exists**
  - It lets humans review the current sprint review branch from a review
    worktree while agents continue to own the sprint workflow branch.
