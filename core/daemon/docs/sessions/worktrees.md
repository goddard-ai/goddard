# Session Worktrees

> A session worktree is an optional isolated Git checkout that the daemon attaches to one session. This page explains when it is created, what it protects, and why cleanup is separate from session completion.

- **Core idea**
  - A session worktree is an optional isolated linked Git worktree managed by the daemon for a session.
  - It lets daemon-managed work happen outside the caller's primary checkout.

- **When used**
  - A caller can opt into isolated worktree behavior for a fresh session.
  - Higher-level daemon workflows can enable worktrees automatically when isolation is part of their contract.
  - A persisted session worktree can be reused when loading the same session.
  - Sessions that do not request isolation run in their resolved working directory without gaining daemon worktree cleanup or branch isolation.

- **What changes**
  - The agent starts in the effective working directory inside the isolated worktree.
  - Session records persist worktree metadata so clients can find the attached workspace.
  - The caller's original checkout is not the workspace being mutated by the agent.
  - The worktree gives the session its own checkout state, but repository policy, session tokens, and review workflows still define what the session is allowed to do.
  - Review-session workflows can use the worktree as the agent side of a human review surface.

- **Cleanup**
  - Worktree cleanup is explicit.
  - Session exit does not automatically remove the worktree.
  - Daemon restart does not automatically remove the worktree.
  - Cleanup must respect user work and repository state rather than assuming all generated files are disposable.
  - If cleanup cannot safely remove the worktree, the daemon should leave the workspace inspectable so the user can decide what to keep.
  - Review sessions may also need to unmount before a worktree can be considered fully cleaned up.

- **Boundaries**
  - Worktrees isolate Git checkout state; they do not make unsafe agent behavior safe by themselves.
  - Custom worktree providers are user-scoped executable extension behavior.
  - Repository-local configuration can shape preparation intent but cannot declare custom executable worktree plugins.
  - Related pages: [worktree preparation](./worktree-preparation.md), [configuration refresh](../concepts/configuration-refresh.md), and [review sessions](../collaboration/review-sessions.md).
