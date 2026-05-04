# `sprint-branch finish --task <task>`

- **Question it answers**
  - Is this active task ready for human review?

- **What it does**
  - Marks an active `review` or `next` task as `finished-unreviewed`.
  - Requires the task markdown to contain a complete Review Report.

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

- **Why it exists**
  - It separates "the agent says this task is done" from "the human has
    approved this task."
