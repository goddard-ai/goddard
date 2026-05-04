# `sprint-branch sync`

- **Question it answers**
  - How do I watch the active sprint review branch through the review-sync
    workflow?

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

- **Guardrails**
  - If sprint status is invalid, `sync` reports sprint diagnostics instead of
    starting the watch.

- **Why it exists**
  - It lets humans review the current sprint review branch from a review
    worktree while agents continue to own the sprint workflow branch.
