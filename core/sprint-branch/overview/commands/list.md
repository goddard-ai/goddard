# `sprint-branch list [--all] [-l|--last]`

- **Question it answers**
  - Which sprint branch states are known?

- **What it reports**
  - Active sprints by default.
  - Parked sprints when `--all` is supplied.
  - Sprints in most-recent activity order, with stable name order as the
    fallback for untouched sprints.
  - Only the latest acted-upon sprint when `-l` / `--last` is supplied.
  - For each sprint:
    - Sprint name.
    - Visibility.
    - Review branch.
    - Last activity timestamp when one has been recorded.
  - Diagnostics for state records that cannot be read.

- **What it changes**
  - Nothing.
  - It does not infer a current sprint.
  - It does not require the current directory or branch to identify a sprint.
  - It does not update activity timestamps.

- **Guardrails**
  - Unreadable sprint state appears as diagnostics rather than stopping the
    entire listing.
  - Listing remains a discovery command even when some sprint records are
    unhealthy.

- **Why it exists**
  - It is the low-risk discovery command for choosing a sprint when the current
    directory or branch does not identify one.
