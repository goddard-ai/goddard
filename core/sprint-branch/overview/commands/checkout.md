# `sprint-branch checkout [name]`

- **Question it answers**
  - How can a human inspect the sprint review branch without taking over the
    live branch?

- **What it does**
  - Checks out the selected sprint's review branch as a detached snapshot.
  - Leaves the review branch agent-owned.
  - Gives the human a commit snapshot rather than a live branch to move.

- **Sprint selection**
  - If `name` is omitted, the sprint can be inferred from:
    - The current sprint branch.
    - A `sprints/<name>` working directory.
  - In an interactive terminal, the command can prompt for an active sprint.
  - Non-interactive callers must provide `name` when no strong context exists.

- **What it changes**
  - The current checkout becomes a detached review snapshot.
  - Sprint branches and sprint state are not advanced.

- **Guardrails**
  - The working tree must be clean before switching snapshots.
  - `--dry-run` shows which sprint and review branch would be checked out.

- **Why it exists**
  - Human review needs a stable, inspectable snapshot.
  - Sprint branches remain controlled by workflow commands.
