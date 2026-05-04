# `sprint-branch land <target> [name]`

- **Question it answers**
  - How does finalized sprint work enter the target branch?

- **Inputs and selection**
  - `<target>` is the branch that should receive finalized sprint content.
  - `name` selects the sprint explicitly.
  - If `name` is omitted, the sprint can be inferred from strong local context.
  - In an interactive terminal, the command can prompt for an active sprint.
  - Non-interactive callers must provide enough context to select one sprint.
  - Parked sprints can be landed by explicit or strong-context selection.
    - Prompted default selection only offers active sprints.

- **What it does**
  - Fast-forwards a human-selected target branch to finalized sprint review
    content.
  - Common target: `main`.

- **What it changes**
  - The target branch moves to the finalized sprint content.
  - Sprint branches and sprint state are not deleted.
  - It does not run cleanup.

- **Finalized-sprint requirements**
  - No task may still be assigned to `review` or `next`.
  - No task may remain `finished-unreviewed`.
  - No sprint conflict may be recorded.
  - No interrupted sprint stash may remain active.
  - `review` and `approved` must represent the same finalized content.
  - `next`, if present, must not contain different work.
  - The target branch must exist.
  - The target branch must not itself be a sprint branch.
  - The target must be able to fast-forward to the finalized review content.
  - The working tree must be clean.

- **Interactive and dry-run behavior**
  - Real execution is a human operation and requires interactive confirmation.
  - Real execution is not available in JSON or non-interactive mode.
  - Non-interactive JSON output is available for `--dry-run` inspection.
  - Dry run reports the target movement without moving the target branch.

- **Why it exists**
  - It separates final human merge authority from agent workflow approval.
  - Agents can prepare a sprint, but landing remains a deliberate human
    operation.
