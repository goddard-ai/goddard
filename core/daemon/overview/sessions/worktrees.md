# Session Worktrees

- **Core idea**
  - A session worktree is an optional isolated linked Git worktree managed by the daemon for a session.
  - It lets daemon-managed work happen outside the caller's primary checkout.

- **When used**
  - A caller can opt into isolated worktree behavior for a fresh session.
  - Higher-level daemon workflows can enable worktrees automatically when isolation is part of their contract.
  - A persisted session worktree can be reused when loading the same session.

- **What changes**
  - The agent starts in the effective working directory inside the isolated worktree.
  - Session records persist worktree metadata so clients can find the attached workspace.
  - The caller's original checkout is not the workspace being mutated by the agent.

- **Cleanup**
  - Worktree cleanup is explicit.
  - Session exit does not automatically remove the worktree.
  - Daemon restart does not automatically remove the worktree.

- **Boundaries**
  - Worktrees isolate Git checkout state; they do not make unsafe agent behavior safe by themselves.
  - Custom worktree providers are user-scoped executable extension behavior.
  - Repository-local configuration can shape preparation intent but cannot declare custom executable worktree plugins.
