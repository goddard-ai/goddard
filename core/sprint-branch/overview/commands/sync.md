# `sprint-branch sync [-l|--last]`

- **Question it answers**
  - How do I watch the active sprint review branch through the review-sync
    workflow?

- **Sprint selection**
  - The sprint comes from normal sprint resolution, `-l` / `--last`, or an
    explicit `--sprint`.
  - Non-interactive callers need a sprint argument, `-l` after recorded
    activity, or strong local context.
  - `-l` / `--last` selects the most recently acted-upon sprint.

- **What it does**
  - Resolves the active sprint.
  - Validates sprint status.
  - Starts a `review-sync` watch session for the sprint's review branch.
  - Surfaces `review-sync` results while running.
  - Exits with the review-sync outcome.

- **What it changes**
  - It delegates to the separate `review-sync` workflow.
  - `review-sync` keeps a disposable review branch aligned with the agent-owned
    branch while preserving human review edits.
  - It does not advance sprint task state by itself.
  - It records the selected sprint's private `lastActedAt` timestamp after
    sprint status is readable.

- **Guardrails**
  - If sprint status is invalid, `sync` reports sprint diagnostics instead of
    starting the watch.
  - The sprint review branch is the agent branch passed into `review-sync`.
  - The command exits with the `review-sync` outcome when the watch session
    starts.

- **Why it exists**
  - It lets humans review the current sprint review branch from a review
    worktree while agents continue to own the sprint workflow branch.
