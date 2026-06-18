# `review-sync watch [agent-branch]`

> The review-sync watch [agent-branch] command is part of the local review-sync workflow. This page explains when to use it, what review state it may change, and what guardrails apply.

- **Question it answers**
  - How can review-sync keep the human review worktree current while agent and
    review changes continue?

- **Inputs and selection**
  - `watch` runs from the human review worktree, not the agent worktree.
  - If the starting checkout is attached to a branch, that branch must not be a `review-sync/*` branch.
  - With `[agent-branch]`, `watch` starts or reuses the session for that branch before watching.
  - Without an agent branch, `watch` infers an existing session from the current
    review worktree.
  - `--verbose` adds progress diagnostics about session resolution, watched
    changes, sync decisions, and cleanup.

- **What it does**
  - Watches the agent worktree, review worktree, and relevant branch movement.
  - Runs `sync` after meaningful changes settle.
  - Syncs review-side commits when they record file content that is already
    rendered in the review worktree.
  - Promotes accepted clean review commits, such as cherry-picks, onto the
    agent branch when their content matches the synchronized agent content.
  - Reactivates a paused session before watching.
  - Emits command results while it runs so wrappers can surface starts, syncs,
    warnings, and final stop status.

- **What it changes**
  - Everything `start` may change when an explicit agent branch starts or reuses a session.
  - Everything `sync` may change during each completed sync cycle.
  - The review branch while preparing from the agent branch ref, and the review
    worktree while refreshing from it.
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
  - It tries to restore the review worktree checkout that was active at startup.
  - Before restoring the startup checkout, it discards rendered baseline dirt
    when the review worktree has no unapplied human patch.
  - Immediately before exit, it deletes the disposable `review-sync/*` review
    branch when there is no unapplied human patch that must stay visible.
  - If cleanup cannot fully complete, the final result reports what remains for the user to handle.
  - If watch stops before a durable session is ready, it tries to undo any safe
    review-branch preview checkout.

- **Guardrails**
  - It preserves local review work instead of overwriting it during waiting or cleanup.
  - It does not run sync while the recorded agent worktree is on the wrong branch.
  - It surfaces rejected human patches through normal `sync` results.
  - It is safe to restart after exit; a paused session can be reactivated by a
    later `watch`, `start`, or `resume`.
