# `sprint-branch view [--task <task>] [-l|--last]`

- **Question it answers**
  - What should a human read to approve this task?

- **Sprint selection**
  - The sprint comes from normal sprint resolution, `-l` / `--last`, or an
    explicit `--sprint`.
  - Non-interactive callers need a sprint argument, `-l` after recorded
    activity, or strong local context.
  - `-l` / `--last` selects the most recently acted-upon sprint.

- **What it reports**
  - The concise approval view for a finished task.
  - By default, the task currently assigned to `review`.
  - With `--task`, a specific finished task by task-file stem or matching task
    filename.
  - The approval view includes:
    - Task identity.
    - Current task state.
    - Review branch name.
    - Approved branch name.
    - Recommended diff command.
    - The task's Review Report.
  - The task markdown is read from the sprint's recorded working tree, so the
    approval packet reflects the sprint plan text the agent prepared.

- **What it changes**
  - It does not change branches, task state, review state, or working tree
    files.
  - It records the selected sprint's private `lastActedAt` timestamp.

- **Guardrails**
  - By default, a review task must be recorded.
  - The selected task must exist in sprint state.
  - The selected task must be marked `finished-unreviewed`.
  - Explicit task selection can view a finished task even when it is not the
    current review task.
  - The task markdown must contain a complete Review Report with:
    - `Plain-English Summary`.
    - `How To Verify Without Reading Code`.
    - `Agent Verification`.
    - `Approval Questions`.
    - `Known Limits`.

- **Why it exists**
  - It creates a durable review handoff.
  - Humans do not need to infer task intent from code or branch names alone.
