# `review-sync` Overview

- **Purpose**
  - This directory describes what `review-sync` supports at a conceptual level.
  - It is written for agents and humans, including reviewers who need to
    understand the review loop without reading source code.
  - It intentionally avoids implementation details:
    - Supported outcomes, guardrails, ownership boundaries, and recovery paths belong here.
    - Helper functions, private schemas, exact diagnostics, storage mechanics,
      and execution order do not.

- **Start here**
  - [Review sync model](./model.md)
    - Worktree and branch roles.
    - Review branch naming.
    - Human patch outcomes.
    - Session inference and guardrails.
    - Recovery states.
  - [Standard review workflow](./standard-review-workflow.md)
    - Normal setup and review loop.
    - Agent-to-review and human-to-agent changes.
    - Watch mode.
    - Conflict and pause recovery.

- **Setup and inspection**
  - [`start`](./commands/start.md)
    - Create or reuse the review session for one agent branch.
    - Audience: humans or agents preparing a separate review worktree.
    - Mutates: review branch checkout, review session state, and possibly accepted human edits.
  - [`status`](./commands/status.md)
    - Inspect the active review-sync session and saved patch counts.
    - Audience: humans, agents, and machine consumers.
    - Mutates: nothing.

- **Synchronization**
  - [`sync`](./commands/sync.md)
    - Run one review-sync cycle between the agent and review worktrees.
    - Audience: humans or agents after either side changes.
    - Mutates: accepted agent-side files, review branch contents, and session state.
  - [`watch`](./commands/watch.md)
    - Keep syncing when either worktree or the agent branch changes.
    - Audience: humans reviewing live agent work.
    - Mutates: delegates to `start`, `sync`, review-branch preparation, and
      session pause-on-exit behavior.

- **Session control**
  - [`pause`](./commands/pause.md)
    - Stop future sync mutations for the inferred session.
    - Audience: humans or agents who need a temporary review-sync stop.
    - Mutates: session pause state only.
  - [`resume`](./commands/resume.md)
    - Re-enable sync mutations without applying changes immediately.
    - Audience: humans or agents continuing a paused review loop.
    - Mutates: session pause state only.
