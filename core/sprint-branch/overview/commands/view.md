# `sprint-branch view [--task <task>]`

- **Question it answers**
  - What should a human read to approve this task?

- **What it reports**
  - The concise approval view for a finished task.
  - By default, the task currently assigned to `review`.
  - With `--task`, a specific task by task file stem.
  - The approval view includes:
    - Task identity.
    - Review branch name.
    - Approved branch name.
    - Recommended diff command.
    - The task's Review Report.

- **What it changes**
  - Nothing.

- **Guardrails**
  - The selected task must exist in sprint state.
  - The selected task must be marked `finished-unreviewed`.
  - The task markdown must contain a complete Review Report with:
    - `Plain-English Summary`.
    - `How To Verify Without Reading Code`.
    - `Agent Verification`.
    - `Approval Questions`.
    - `Known Limits`.

- **Why it exists**
  - It creates a durable review handoff.
  - Humans do not need to infer task intent from code or branch names alone.
