# Review Sync Commands

Review-sync commands set up, inspect, synchronize, pause, and resume a local review workflow. This index links each command to the question it answers and the review state it may change.

## Setup and inspection

- [`start`](./start.md)
  - Create or reuse the review session for one agent branch.
- [`status`](./status.md)
  - Inspect the active review-sync session and saved patch counts.

## Synchronization

- [`sync`](./sync.md)
  - Run one review-sync cycle between the agent and review worktrees.
- [`watch`](./watch.md)
  - Keep syncing when either worktree or the agent branch changes.

## Session control

- [`pause`](./pause.md)
  - Stop future sync mutations for the inferred session.
- [`resume`](./resume.md)
  - Re-enable sync mutations without applying changes immediately.
