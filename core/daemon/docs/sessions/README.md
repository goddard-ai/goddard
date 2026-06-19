# Sessions

> A daemon session is one Goddard agent conversation or task with local identity, status, history, diagnostics, and optional workspace isolation. This section breaks session behavior into pages that can be read independently.

## Purpose

- This folder explains daemon-managed sessions as user-visible concepts.
- It separates launch, access, live control, history, and worktree behavior so users can find the concept they need directly.

## Start here

- [Lifecycle](./lifecycle.md)
  - Creation, active sessions, one-shot sessions, completion, shutdown, reconnection, and history-only records.
- [Launch preview and leases](./launch-preview-and-leases.md)
  - Capability discovery before durable session creation and safe abandoned-launch release.
- [Session tokens](./session-tokens.md)
  - Narrow session authority for daemon-launched tools.

## During a session

- [History and diagnostics](./history-and-diagnostics.md)
  - Transcript history, live streams, lifecycle streams, diagnostics, and restart inspection.
- [Cancellation and steering](./cancellation-and-steering.md)
  - Cancelling active work, aborted queued prompts, and cancel-and-reprompt behavior.
- [Composer suggestions](./composer-suggestions.md)
  - Session-scoped and draft command suggestions.

## Workspace isolation

- [Worktrees](./worktrees.md)
  - Optional isolated linked Git worktrees for daemon-managed sessions.
- [Worktree preparation](./worktree-preparation.md)
  - Fresh worktree artifact seeding, bootstrap, and trust boundaries.
