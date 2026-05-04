# `sprint-branch approve`

- **Question it answers**
  - How does human-approved review work become the new approved boundary?

- **Inputs and selection**
  - Uses [standard sprint selection](../sprint-selection.md).
  - Approval always applies to the task currently recorded on `review`.

- **What it does**
  - Promotes the current review task into `approved`.
  - If no `next` task exists:
    - Advances `approved` to the review content.
    - Records the task as approved.
    - Clears the review task.
    - Leaves `review` checked out and ready for the next start.
  - If a `next` task exists:
    - Ensures next work is based on the review content.
    - Advances `approved`.
    - Records the reviewed task as approved.
    - Rolls the next task forward into review.
    - Empties the `next` slot.
    - Leaves `review` checked out for the rolled-forward task.

- **What it changes**
  - Approved branch content.
  - Review branch role.
  - Possibly next branch content.
  - Sprint task state.

- **Guardrails**
  - The review task must exist.
  - The review task must be marked `finished-unreviewed`.
  - The review task must have a complete Review Report.
  - The review branch must be based on the current approved branch.
  - The working tree must be clean.
  - If an approval conflict occurs:
    - The approved branch is not advanced past the safe boundary.
    - The reviewed task is not recorded as approved.
    - `approve` must be retried after conflict resolution.

- **Dry run**
  - Reports how the queue would move.
  - Does not move sprint branches.
  - Does not switch checkout state.
  - Does not update task state.

- **Why it exists**
  - Approval is the transition from "ready for human review" to "accepted as the
    baseline for future sprint work."
