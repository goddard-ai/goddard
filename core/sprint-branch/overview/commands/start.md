# `sprint-branch start --task <task> [-l|--last]`

- **Question it answers**
  - Which branch should the agent use for the next task?

- **Inputs and selection**
  - The sprint comes from normal sprint resolution, `-l` / `--last`, or an
    explicit `--sprint`.
  - `-l` / `--last` selects the most recently acted-upon sprint.
  - `--task <task>` identifies a task file in the sprint folder.
    - The task may be named by its task-file stem or matching task filename.
    - The task must still be the next unassigned task in sprint task-file order
      unless it is already assigned to a rolling branch.

- **What it does**
  - Assigns the requested task to the correct rolling branch.
  - Branch choice:
    - If no task is on `review`, the task starts on `review`.
      - `review` is reset to the current `approved` boundary before work begins.
    - If the task is already on `review`, the command continues that review
      task.
    - If `review` is occupied and `next` is empty, the task starts as
      work-ahead on `next`.
      - `next` is reset to the current `review` boundary before work-ahead
        begins.
    - If the task is already on `next`, the command continues that next task.

- **What it changes**
  - May move the branch that represents the requested task.
  - Checks out the relevant sprint branch.
  - Updates sprint task state.
  - Records the sprint's private `lastActedAt` timestamp.

- **Guardrails**
  - The requested task must be the next unassigned task in sprint task-file
    order.
  - The workflow supports at most two active tasks:
    - One on `review`.
    - One on `next`.
  - Starting a third task is blocked until review, feedback, resume, or approval
    moves the queue forward.
  - Requires a clean working tree because it may switch branches and update
    task-to-branch assignment.
  - Is blocked while another sprint transition is waiting for conflict
    recovery.

- **Dry run**
  - Reports which branch would receive the task.
  - Does not move branches.
  - Does not switch checkout state.
  - Does not update task state.

- **Why it exists**
  - It keeps sprint work ordered.
  - It prevents agents from inventing extra branch roles that humans cannot
    review predictably.
