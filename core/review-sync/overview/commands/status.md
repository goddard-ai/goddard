# `review-sync status`

- **Question it answers**
  - What is the current review-sync session state?

- **Session selection**
  - The session is inferred from the current worktree or checked-out branch.
  - The command may run from either the recorded agent worktree or the recorded review worktree.
  - If multiple saved sessions match, the command refuses to guess and reports recovery direction.

- **What it reports**
  - Agent and review worktrees.
  - Agent and review branches.
  - Whether the session is paused.
  - The latest known sync outcome.
  - Counts of accepted and rejected patches.
  - Snapshot identities used by callers that need to compare session state.

- **Machine-readable output**
  - `--json` prints the same session information as structured JSON.
  - JSON mode is intended for agents and wrappers that need stable fields
    instead of human-readable text.

- **What it changes**
  - Nothing.
  - It does not move branches, apply patches, refresh the review worktree, pause, or resume.

- **Guardrails and recovery**
  - If no session matches the current worktree, run from a recorded worktree or
    start a session first.
  - If multiple sessions match, keep the intended session and move stale saved
    sessions aside only after checking their saved patch inventory.
  - A paused status means later sync mutations are blocked until `resume`,
    `start`, or `watch` reactivates the session.
