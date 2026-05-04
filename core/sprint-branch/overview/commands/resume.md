# `sprint-branch resume`

- **Question it answers**
  - How does the agent return to interrupted or dependent work-ahead after
    feedback?

- **What it does**
  - If a `next` task exists:
    - Returns to `next`.
    - Ensures `next` is based on the latest review content.
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
  - When a resume-related conflict occurs:
    - Sprint state remains at the pre-resume boundary.
    - The command must be retried after conflict resolution.

- **Why it exists**
  - Work-ahead depends on review.
  - After feedback changes review, dependent next work must be brought forward
    before it can safely continue.
