# `sprint-branch finish --task <task>`

`finish` answers "is this active task ready for human review?"

It marks an active `review` or `next` task as `finished-unreviewed`. The task
must already be active, and its task markdown must contain a complete Review
Report. Marking a task finished does not approve the task and does not move any
branch.

At most two tasks can be finished and unreviewed at once. This matches the two
active branch slots and keeps the human review queue bounded.

`finish` does not require a clean working tree because it only records review
readiness after the Review Report is complete.

Why it matters: it separates "the agent says this task is done" from "the human
has approved this task."
