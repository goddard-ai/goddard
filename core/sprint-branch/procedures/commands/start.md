# `sprint-branch start --task <task>`

`start` answers "which branch should the agent use for the next task?"

It assigns the requested task to the correct rolling branch:

- If no task is on `review`, the requested task starts on `review`.
- If the requested task is already on `review`, the command continues that
  review task.
- If `review` is occupied and `next` is empty, the requested task starts as
  work-ahead on `next`.
- If the requested task is already on `next`, the command continues that next
  task.

The requested task must be the next unassigned task in sprint task-file order.
The workflow supports at most two active tasks at once: one on `review` and one
on `next`. Starting a third task is blocked until review, feedback, resume, or
approval moves the queue forward.

`start` requires a clean working tree because it may switch to a sprint branch
and update which branch represents the requested task.

Why it matters: it keeps sprint work ordered and prevents agents from inventing
extra branch roles that humans cannot review predictably.
