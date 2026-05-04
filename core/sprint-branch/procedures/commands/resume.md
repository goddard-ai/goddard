# `sprint-branch resume`

`resume` answers "how does the agent return to interrupted or dependent
work-ahead after feedback?"

If a `next` task exists, `resume` returns to `next`, makes sure it is based on
the latest review content, and restores any recorded interrupted work for the
current next task. If no `next` task exists, it returns to `review`.

`resume` normally requires a clean working tree. The exception is the final
step of resolving a previously recorded resume conflict, where the working tree
may contain the resolved files needed to complete the retry.

When a resume-related conflict occurs, the sprint state remains at the
pre-resume boundary until the conflict is resolved and `resume` is retried.

Why it matters: work-ahead depends on review. After feedback changes review,
dependent next work must be brought forward before it can safely continue.
