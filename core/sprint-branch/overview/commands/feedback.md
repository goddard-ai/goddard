# `sprint-branch feedback`

- **Question it answers**
  - How can the agent stop work-ahead and return to the review branch for
    human-requested changes?

- **Inputs and selection**
  - Uses [standard sprint selection](../sprint-selection.md).

- **What it does**
  - Prepares the review branch for feedback work.
  - If the current checkout is the sprint `next` branch and has local edits:
    - Preserves the interrupted work.
    - Records it as active sprint work to resume later.
    - Includes untracked work in the preserved interruption.
  - If the current checkout is the sprint `next` branch and is clean:
    - Returns to `review` without recording interrupted work.
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
  - If the `next` branch is checked out, a next task must be recorded.
  - Is blocked while another sprint transition is waiting for conflict
    recovery.
  - If preserving interrupted work fails:
    - The working tree remains in place.
    - Sprint state is not updated.
  - If switching back to `review` fails:
    - Sprint state is not updated.

- **Dry run**
  - Reports whether interrupted work would be preserved.
  - Reports that the working context would move to `review`.
  - Does not preserve work, switch branches, or update sprint state.

- **Why it exists**
  - Human feedback often arrives while an agent is working ahead.
  - The command protects work-ahead changes while making the review branch
    available for the feedback response.
