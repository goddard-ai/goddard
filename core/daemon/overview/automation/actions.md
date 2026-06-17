# Actions

- **Core idea**
  - An action is a reusable one-shot execution definition.
  - Running a named action creates a daemon-managed session for that action.

- **Resolution**
  - The daemon resolves the action's prompt and configuration from applicable configuration roots.
  - Action-specific configuration inherits global and repository baseline configuration before action defaults apply.
  - Runtime input remains ephemeral unless explicitly persisted elsewhere.

- **Session behavior**
  - The resulting session can use ordinary session history, diagnostics, model and config options, inbox reporting, and optional worktree isolation.
  - A named action is intended for one-shot work rather than a long-running runtime.

- **Boundaries**
  - Action defaults belong in machine-readable configuration, not prompt metadata.
  - Running an action does not create a separate runtime ownership model outside sessions.
