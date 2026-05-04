# `review-sync pause`

- **Question it answers**
  - How can sync mutations be stopped without tearing down the review session?

- **Session selection**
  - The session is inferred from the current worktree or checked-out branch.
  - The command may run from either the recorded agent worktree or the recorded review worktree.

- **What it does**
  - Marks the session as paused.
  - Records that future sync attempts should not mutate files until the session is resumed.

- **What it changes**
  - Session pause state.
  - Last-sync status for the session.

- **What it never changes**
  - It does not apply or reject patches.
  - It does not refresh the review branch.
  - It does not discard human or agent worktree edits.
  - It does not delete the saved relationship between the agent and review worktrees.

- **Common next actions**
  - Run `status` to confirm the session is paused.
  - Run `resume` when sync mutations should be allowed again.
  - Run `watch` when returning to live review; watch reactivates a paused session before watching.
