# `sprint-branch cleanup <target> [name] [--force]`

- **Question it answers**
  - How do we remove sprint-specific local artifacts after landing?

- **Inputs and selection**
  - `<target>` is the branch that must already contain the finalized sprint
    review commit.
  - Uses [standard sprint selection](../sprint-selection.md).
  - The optional `name` argument is this command's explicit sprint selector.
  - When no sprint can be inferred and cleanup prompts interactively, multiple
    active sprints may be selected and cleaned up together.
  - Explicit `name` and `--last` selection still select one sprint.
  - `--force` allows cleanup when the target does not contain the finalized
    review commit.

- **What it removes**
  - Landed sprint branches.
  - Private sprint state for the selected sprint.
  - The selected sprint's local sprint folder backup.
  - The optional `next` branch when it exists.

- **What it changes**
  - Clean worktrees checked out on sprint branches are detached so those
    branches can be deleted.
  - Detached review snapshots are left in place.
  - Local sprint branches, private state, and the local sprint folder backup are
    removed.
  - The target branch is not advanced by cleanup.

- **Guardrails**
  - The target branch must contain the finalized review commit.
    - With `--force`, this blocker becomes a warning and sprint branches are
      force-deleted.
  - The sprint must be finalized.
  - An obsolete `next` branch accepted during finalization remains a warning
    only while its recorded commit still matches the finalized state.
  - The working tree must be clean.
  - The target branch must exist.
  - The target branch must not itself be a sprint branch.
  - Worktrees checked out on sprint branches must be clean before they are
    detached.
  - Cleanup does not remove worktree directories.

- **Interactive and dry-run behavior**
  - Real execution is a human operation and requires interactive confirmation.
  - Real execution is not available in JSON or non-interactive mode.
  - Non-interactive JSON output is available for `--dry-run` inspection.
  - `--force` does not bypass the interactive confirmation requirement.
  - Multi-sprint selection is available only from the interactive prompt.
  - Dry run reports what would be detached or removed without changing
    worktrees, deleting branches, deleting private sprint state, or deleting the
    sprint folder backup.

- **Why it exists**
  - Cleanup is intentionally separate from landing.
  - A sprint can be landed and inspected before local sprint branches and review
    state are removed.
