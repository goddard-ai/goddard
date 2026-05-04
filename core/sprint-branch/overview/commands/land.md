# `sprint-branch land <target> [name]`

- **Question it answers**
  - How does finalized sprint work enter the target branch?

- **What it does**
  - Fast-forwards a human-selected target branch to finalized sprint review
    content.
  - Common target: `main`.
  - Requires interactive confirmation for real execution.
  - Supports `--dry-run --json` for non-interactive inspection.

- **What it changes**
  - The target branch moves to the finalized sprint content.
  - Sprint branches and sprint state are not deleted.

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

- **Why it exists**
  - It separates final human merge authority from agent workflow approval.
  - Agents can prepare a sprint, but landing remains a deliberate human
    operation.
