# `sprint-branch view [--task <task>]`

`view` answers "what should a human read to approve this task?"

It prints the concise approval view for a finished task. By default, it uses the
task currently assigned to `review`; `--task` selects a specific task by task
file stem. The view includes the task identity, review and approved branch names,
the recommended diff command, and the task's Review Report.

The selected task must exist in sprint state and be marked
`finished-unreviewed`. Its task markdown must contain a complete Review Report
with these sections:

- `Plain-English Summary`
- `How To Verify Without Reading Code`
- `Agent Verification`
- `Approval Questions`
- `Known Limits`

`view` does not change anything.

Why it matters: it creates a durable review handoff. The human does not need to
infer intent from code or branch names alone.
