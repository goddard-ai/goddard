# Review Sessions

> Review sessions connect a daemon-managed session worktree to Goddard's separate review-sync workflow. This page explains how the daemon mounts, runs, and unmounts that review surface for human review.

- **Core idea**
  - Review sessions connect daemon-managed session worktrees to the `review-sync` workflow.
  - They give humans a separate review surface for a daemon-managed session's isolated worktree.
  - The daemon owns mounting, running, and unmounting the review-session runtime for that session worktree.

- **Session worktree dependency**
  - Review sessions require a daemon-managed session worktree.
  - The worktree identifies the repository, agent branch, and session context that review-sync should operate against.
  - If a session has no worktree, review-session operations cannot provide the review-sync mount.

- **Mount**
  - Mounting prepares or reuses the review-sync session for one daemon-managed session worktree.
  - Mounting establishes the review branch and review worktree relationship used by review-sync.
  - A mounted review session can optionally begin its runtime immediately.
  - If mounting cannot establish a safe review relationship, the daemon should report that outcome instead of pretending review-sync is available.

- **Run**
  - Running asks review-sync to perform a sync cycle for the mounted session worktree.
  - The result reflects review-sync outcomes such as accepted work, rejected human patch, paused state, or error state.
  - Clients should present those outcomes as review workflow state, not as direct edits to the daemon session transcript.

- **Unmount**
  - Unmounting stops the review-session relationship for the daemon-managed worktree.
  - It is the explicit cleanup path for the mounted review surface.
  - Unmounting does not delete the underlying daemon session or its attached worktree by itself.

- **Recovery**
  - Daemon reconciliation can clean up mounted review sessions when the associated daemon session worktree no longer has live session ownership.
  - Review-sync guardrails still apply to agent and review worktree branches.
  - When recovery changes mounted state, clients should reload review-session state from the daemon before offering another run action.

- **Boundaries**
  - Review sessions do not replace the `review-sync` conceptual contract.
  - They are the daemon-owned bridge between a session worktree and that existing review workflow.
  - Review branch content is a review surface; the durable daemon session and underlying agent branch retain their own roles.
  - Related pages: [session worktrees](../sessions/worktrees.md), [pull requests](./pull-requests.md), and the review-sync docs outside the daemon package.
