# `sprint-branch list [--all]`

- **Question it answers**
  - Which sprint branch states are known?

- **What it reports**
  - Active sprints by default.
  - Parked sprints when `--all` is supplied.
  - For each sprint:
    - Sprint name.
    - Visibility.
    - Review branch.

- **What it changes**
  - Nothing.
  - It does not infer a current sprint.

- **Guardrails**
  - Unreadable sprint state appears as diagnostics rather than stopping the
    entire listing.

- **Why it exists**
  - It is the low-risk discovery command for choosing a sprint when the current
    directory or branch does not identify one.
