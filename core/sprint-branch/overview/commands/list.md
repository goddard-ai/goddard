# `sprint-branch list [--all]`

- **Question it answers**
  - Which sprint branch states are known?

- **What it reports**
  - Active sprints by default.
  - Parked sprints when `--all` is supplied.
  - Sprints in stable name order.
  - For each sprint:
    - Sprint name.
    - Visibility.
    - Review branch.
  - Diagnostics for state records that cannot be read.

- **What it changes**
  - Nothing.
  - It does not infer a current sprint.
  - It does not require the current directory or branch to identify a sprint.

- **Guardrails**
  - Unreadable sprint state appears as diagnostics rather than stopping the
    entire listing.
  - Listing remains a discovery command even when some sprint records are
    unhealthy.

- **Why it exists**
  - It is the low-risk discovery command for choosing a sprint when the current
    directory or branch does not identify one.
