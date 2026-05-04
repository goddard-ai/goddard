# `review-sync resume`

- **Question it answers**
  - How can a paused review-sync session allow sync mutations again?

- **Session selection**
  - The session is inferred from the current worktree or checked-out branch.
  - The command may run from either the recorded agent worktree or the recorded review worktree.

- **What it does**
  - Clears the paused state for the inferred session.
  - Leaves the session ready for a later `sync`, `start`, or `watch`.

- **What it changes**
  - Session pause state only.

- **What it never changes**
  - It does not apply pending human edits.
  - It does not refresh the review worktree.
  - It does not switch branches.
  - It does not create or reject patch files.

- **Common next actions**
  - Run `sync` to process current worktree changes once.
  - Run `watch` to continue live synchronization.
  - Run `status` if you need to confirm the resumed state before mutating.
