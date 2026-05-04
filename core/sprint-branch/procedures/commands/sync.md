# `sprint-branch sync`

`sync` answers "how do I watch the active sprint review branch through the
review-sync workflow?"

It resolves the active sprint, validates the sprint status, and starts a
`review-sync` watch session for the sprint's review branch. `review-sync` is the
separate Git-only review workflow that keeps a disposable review branch aligned
with the agent-owned branch while preserving human review edits.

If sprint status is invalid, `sync` reports the sprint diagnostics instead of
starting the watch. While running, it surfaces `review-sync` results and exits
with the review-sync outcome.

Why it matters: it lets humans review the current sprint review branch from a
review worktree while agents continue to own the sprint workflow branch.
