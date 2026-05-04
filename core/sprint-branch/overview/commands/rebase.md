# `sprint-branch rebase <target>`

- **Question it answers**
  - How can the whole sprint branch stack move onto a newer base?

- **What it does**
  - Moves recorded sprint branches onto a target ref.
  - Preserves the rolling relationship between:
    - `approved`.
    - `review`.
    - `next`.
  - Records the new base only after every relevant sprint branch has moved
    successfully.

- **What it changes**
  - Recorded sprint branch commits.
  - Sprint base state after all branch movement succeeds.

- **Guardrails**
  - The target must exist.
  - The target must not be one of the sprint branches.
  - The target must share history with the current approved branch.
  - The working tree must be clean.
  - No interrupted sprint stashes may be active.
  - If a conflict interrupts the transition:
    - The prior base remains recorded.
    - `rebase` can be retried after conflict resolution.

- **Why it exists**
  - It keeps long-running sprint work current with the target branch.
  - It avoids collapsing the review boundary or losing the work-ahead
    relationship.
