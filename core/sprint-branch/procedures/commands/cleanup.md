# `sprint-branch cleanup <target> [name]`

`cleanup` answers "how do we remove sprint-specific local artifacts after
landing?"

It removes landed sprint branches, clean associated review worktrees, and the
private sprint state for the selected sprint. It is a human command and requires
interactive confirmation for real execution. `--dry-run --json` is supported for
non-interactive inspection.

The target branch must contain the finalized review commit before cleanup can
proceed. Cleanup also requires the sprint to be finalized, the working tree to
be clean, and the current branch not to be one of the branches that would be
deleted.

Why it matters: cleanup is intentionally separate from landing. A sprint can be
landed and inspected before local sprint branches and review worktrees are
removed.
