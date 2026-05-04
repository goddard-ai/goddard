# `sprint-branch resume`

- **Question it answers**
  - How does the agent return to interrupted or dependent work-ahead after
    feedback?

- **Inputs and selection**
  - Uses [standard sprint selection](../sprint-selection.md).

- **What it does**
  - If a `next` task exists:
    - Returns to `next`.
    - Ensures `next` is based on the latest review content when review changed.
    - Restores recorded interrupted work for the current next task.
  - If no `next` task exists:
    - Returns to `review`.

- **What it changes**
  - Checkout state.
  - Dependent `next` work when it needs to be brought forward.
  - Recorded interrupted work when it is restored or cleared.

- **Guardrails**
  - Normally requires a clean working tree.
  - The exception is the final step of resolving a previously recorded resume
    conflict, where resolved files may remain in the working tree.
  - If a `next` task is recorded, the `next` branch must still exist.
  - When a resume-related conflict occurs:
    - Sprint state remains at the pre-resume boundary.
    - The command must be retried after conflict resolution.
  - If the interrupted work was already applied and the user has resolved the
    conflict:
    - Retrying `resume` clears the recorded interruption.
    - The restored work may remain as local edits for the agent to continue.

- **Dry run**
  - Reports whether the command would return to `review` or `next`.
  - Reports whether dependent work would be brought forward.
  - Reports whether recorded interrupted work would be restored.
  - Does not switch branches, move branch content, restore work, or update
    sprint state.

- **Why it exists**
  - Work-ahead depends on review.
  - After feedback changes review, dependent next work must be brought forward
    before it can safely continue.
