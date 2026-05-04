# `review-sync watch [agent-branch]`

- **Question it answers**
  - How can review-sync keep the human review worktree current while agent and
    review changes continue?

- **Inputs and selection**
  - With `[agent-branch]`, `watch` starts or reuses the session for that branch before watching.
  - Without an agent branch, `watch` infers an existing session from the current
    worktree or checked-out branch.
  - `--verbose` adds progress diagnostics about session resolution, watched
    changes, sync decisions, and cleanup.

- **What it does**
  - Watches the agent worktree, review worktree, and relevant branch movement.
  - Runs `sync` after meaningful changes settle.
  - Reactivates a paused session before watching.
  - Emits command results while it runs so wrappers can surface starts, syncs,
    warnings, and final stop status.

- **What it changes**
  - Everything `start` may change when an explicit agent branch starts or reuses a session.
  - Everything `sync` may change during each completed sync cycle.
  - The review branch while preparing or refreshing from the agent branch ref.
  - Session pause state when watch exits.
  - The review worktree checkout when watch can safely restore the branch that
    was active at startup.

- **Waiting for agent checkout**
  - If an explicit agent branch is not currently checked out in an agent
    worktree, `watch` waits instead of failing immediately.
  - While waiting, it may check out or refresh the derived review branch from the agent branch ref.
  - It does not do that preparation when the review worktree has local edits
    that would be overwritten.
  - Human commits or dirty edits already on the review side are preserved and
    can be applied after the agent checkout becomes available.

- **Temporary agent branch mismatch**
  - If a saved session exists but the recorded agent worktree is temporarily on
    another branch, `watch` waits for the recorded agent branch to return.
  - While waiting, it can refresh the review worktree from the agent branch ref
    when there is no unapplied human patch.
  - If human edits would be overwritten by that refresh, `watch` leaves them in
    place and reports a warning.

- **Exit behavior**
  - When stopped after a session is active, `watch` pauses the session.
  - If `watch` started from the review worktree on another branch, it tries to
    restore that starting branch.
  - If cleanup cannot fully complete, the final result reports what remains for the user to handle.
  - If watch stops before a durable session is ready, it tries to undo any safe
    review-branch preview checkout.

- **Guardrails**
  - It preserves local review work instead of overwriting it during waiting or cleanup.
  - It does not run sync while the recorded agent worktree is on the wrong branch.
  - It surfaces rejected human patches through normal `sync` results.
  - It is safe to restart after exit; a paused session can be reactivated by a
    later `watch`, `start`, or `resume`.
