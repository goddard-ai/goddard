# Actions and Loops

- **Named actions**
  - An action is a reusable one-shot execution definition.
  - Running a named action resolves its prompt and configuration from the applicable configuration roots.
  - The daemon creates a daemon-managed session for the action.
  - The action's session can use normal session behavior such as history, diagnostics, model and config options, inbox reporting, and optional worktree isolation when configured.

- **Action configuration**
  - Action defaults are machine-readable configuration, not prompt metadata.
  - Action-specific configuration inherits global and repository baseline configuration before the action's own defaults apply.
  - Runtime input remains ephemeral unless a user explicitly persists new configuration elsewhere.

- **Loops**
  - A loop is a named automation runtime that can be started and inspected through the daemon.
  - Starting a loop creates or reuses the daemon-owned loop runtime for the requested context.
  - Clients can fetch one loop runtime, list loop summaries, or shut down a loop.
  - Loop runtime state is owned by the daemon; clients should not create parallel watcher state for the same loop.

- **Loop configuration**
  - Loops resolve persisted configuration before runtime behavior begins.
  - A loop may be represented by prompt content or by a richer packaged definition.
  - Persisted loop defaults live in machine-readable configuration associated with the loop.

- **Guardrails**
  - Actions are for one-shot execution.
  - Loops are for reusable runtime behavior that may continue until shut down.
  - Configuration resolution happens before execution begins so active work runs against a stable view of intent.
  - Invalid persisted configuration should not replace the daemon's last valid behavior for future resolutions.
