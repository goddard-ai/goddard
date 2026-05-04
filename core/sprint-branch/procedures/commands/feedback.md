# `sprint-branch feedback`

`feedback` answers "how can the agent stop work-ahead and return to the review
branch for human-requested changes?"

It prepares the review branch for feedback work. If the current branch is
`next` and has local edits, the command preserves that interrupted work and
records it as active sprint work to resume later. It then moves the working
context back to `review`.

Dirty work outside the recorded `next` branch is blocked. The command is allowed
to preserve dirty `next` work because that is the interruption it is designed to
handle.

Task assignments do not move during feedback: the review task remains the
review task, and the next task remains the next task.

Why it matters: human feedback often arrives while an agent is working ahead.
This command protects the work-ahead changes while making the review branch
available for the feedback response.
