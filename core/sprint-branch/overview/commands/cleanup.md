# `sprint-branch cleanup <target> [name]`

- **Question it answers**
  - How do we remove sprint-specific local artifacts after landing?

- **What it removes**
  - Landed sprint branches.
  - Clean associated review worktrees.
  - Private sprint state for the selected sprint.

- **What it changes**
  - Local sprint branches, associated worktrees, and private state are removed.
  - The target branch is not advanced by cleanup.

- **Guardrails**
  - Real execution requires interactive confirmation.
  - `--dry-run --json` is supported for non-interactive inspection.
  - The target branch must contain the finalized review commit.
  - The sprint must be finalized.
  - The working tree must be clean.
  - The current branch must not be one of the branches that would be deleted.

- **Why it exists**
  - Cleanup is intentionally separate from landing.
  - A sprint can be landed and inspected before local sprint branches and review
    worktrees are removed.
