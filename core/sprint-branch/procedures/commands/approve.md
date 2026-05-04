# `sprint-branch approve`

`approve` answers "how does human-approved review work become the new approved
boundary?"

It promotes the current review task into `approved`. The review task must exist,
be marked `finished-unreviewed`, have a complete Review Report, and be based on
the current approved branch. The working tree must be clean.

If no `next` task exists, approval advances `approved` to the review content,
records the task as approved, clears the review task, and leaves review ready
for the next start.

If a `next` task exists, approval first ensures the next work is based on the
review content, then advances `approved`, records the reviewed task as approved,
and rolls the next task forward so it becomes the new review task. The `next`
slot becomes empty.

When an approval conflict occurs, the reviewed task is not recorded as approved
until the conflict is resolved and `approve` is retried.

Why it matters: approval is the transition from "ready for human review" to
"accepted as the baseline for future sprint work."
