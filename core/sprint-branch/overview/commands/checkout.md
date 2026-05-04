# `sprint-branch checkout [name]`

- **Question it answers**
  - How can a human inspect the sprint review branch without taking over the
    live branch?

- **What it does**
  - Checks out the selected sprint's review branch as a detached snapshot.
  - Leaves the review branch agent-owned.
  - Gives the human a commit snapshot rather than a live branch to move.

- **Sprint selection**
  - If `name` is provided, that sprint is selected explicitly.
  - If `name` is omitted, the sprint can be inferred from strong local context:
    - The current sprint branch.
    - A `sprints/<name>` working directory.
  - In an interactive terminal, the command can prompt for an active sprint.
  - Non-interactive callers must provide `name` when no strong context exists.
  - Parked sprints can be checked out by explicit or strong-context selection.
    - Prompted default selection only offers active sprints.

- **What it changes**
  - The current checkout becomes a detached review snapshot.
  - Sprint branches and sprint state are not advanced.

- **Guardrails**
  - The working tree must be clean before switching snapshots.
  - The selected sprint state must exist and be readable.
  - The selected sprint's review branch must exist.
  - `--dry-run` shows which sprint and review branch would be checked out.

- **After checkout**
  - The snapshot represents the review branch commit at checkout time.
  - If the agent changes review later, run `checkout` again to inspect a fresh
    snapshot.

- **Why it exists**
  - Human review needs a stable, inspectable snapshot.
  - Sprint branches remain controlled by workflow commands.
