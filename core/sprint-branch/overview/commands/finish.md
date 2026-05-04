# `sprint-branch finish --task <task>`

- **Question it answers**
  - Is this active task ready for human review?

- **Inputs and selection**
  - Uses [standard sprint selection](../sprint-selection.md).
  - `--task <task>` identifies the active task being marked ready.
    - The task may be named by its task-file stem or matching task filename.

- **What it does**
  - Marks an active `review` or `next` task as `finished-unreviewed`.
  - Requires the task markdown to contain a complete Review Report.
  - If the task is already marked `finished-unreviewed`, the command reports
    that state without adding a duplicate record.

- **What it changes**
  - Sprint task state only.
  - It does not:
    - Approve the task.
    - Move any branch.

- **Guardrails**
  - The task must already be active.
  - At most two tasks can be finished and unreviewed at once.
    - This matches the two active branch slots.
    - This keeps the human review queue bounded.
  - A clean working tree is not required because the command only records review
    readiness after the Review Report is complete.
  - Is blocked while another sprint transition is waiting for conflict
    recovery.

- **Dry run**
  - Reports whether the task would be marked ready for review.
  - Does not update sprint state.

- **Why it exists**
  - It separates "the agent says this task is done" from "the human has
    approved this task."
