# `sprint-branch feedback`

- **Question it answers**
  - How can the agent stop work-ahead and return to the review branch for
    human-requested changes?

- **What it does**
  - Prepares the review branch for feedback work.
  - If the current branch is `next` and has local edits:
    - Preserves the interrupted work.
    - Records it as active sprint work to resume later.
  - Moves the working context back to `review`.

- **What it changes**
  - Checkout state.
  - Possibly recorded interrupted sprint work.
  - Task assignments do not move:
    - The review task remains the review task.
    - The next task remains the next task.

- **Guardrails**
  - Dirty work outside the recorded `next` branch is blocked.
  - Dirty `next` work is allowed because preserving that interruption is the
    command's purpose.

- **Why it exists**
  - Human feedback often arrives while an agent is working ahead.
  - The command protects work-ahead changes while making the review branch
    available for the feedback response.
