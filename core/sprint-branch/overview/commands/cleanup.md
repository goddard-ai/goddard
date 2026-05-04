# `sprint-branch cleanup <target> [name]`

- **Question it answers**
  - How do we remove sprint-specific local artifacts after landing?

- **Inputs and selection**
  - `<target>` is the branch that must already contain the finalized sprint
    review commit.
  - Uses [standard sprint selection](../sprint-selection.md).
  - The optional `name` argument is this command's explicit sprint selector.

- **What it removes**
  - Landed sprint branches.
  - Clean associated review worktrees, including detached review snapshots.
  - Private sprint state for the selected sprint.
  - The optional `next` branch when it exists.

- **What it changes**
  - Local sprint branches, associated worktrees, and private state are removed.
  - The target branch is not advanced by cleanup.

- **Guardrails**
  - The target branch must contain the finalized review commit.
  - The sprint must be finalized.
  - The working tree must be clean.
  - The current branch must not be one of the branches that would be deleted.
  - The target branch must exist.
  - The target branch must not itself be a sprint branch.
  - Associated worktrees must be clean before they are removed.
  - Cleanup will not remove the current worktree.

- **Interactive and dry-run behavior**
  - Real execution is a human operation and requires interactive confirmation.
  - Real execution is not available in JSON or non-interactive mode.
  - Non-interactive JSON output is available for `--dry-run` inspection.
  - Dry run reports what would be removed without deleting branches, worktrees,
    or private sprint state.

- **Why it exists**
  - Cleanup is intentionally separate from landing.
  - A sprint can be landed and inspected before local sprint branches and review
    worktrees are removed.
