# Configuration Refresh

- **Core idea**
  - The daemon owns persisted root-config loading and refresh for long-running local automation.
  - Valid config changes affect future work after the daemon accepts the updated snapshot.
  - Work that already started continues under the configuration resolved for that work.

- **Last-good behavior**
  - The daemon preserves the last valid behavior when a persisted config edit is invalid.
  - Invalid edits must be corrected before they affect future work.
  - This avoids breaking active local automation because a watched config file is temporarily malformed.

- **Configuration scopes**
  - User configuration supplies personal defaults and user-scoped trusted extension references.
  - Repository configuration supplies shared repository intent.
  - Runtime input supplies one-invocation overrides that do not implicitly become persisted preferences.

- **Trust boundary**
  - Repository-local configuration may shape non-executable daemon behavior such as session worktree preparation intent.
  - Repository-local configuration cannot declare custom executable daemon worktree plugins.
  - User-scoped executable extension settings are a separate trust boundary.

- **User-facing outcomes**
  - Saving valid config can change how later sessions, actions, loops, or worktrees resolve.
  - Existing work does not silently change configuration underneath an active run.
  - If config watching degrades, the daemon should continue using accepted valid behavior until it can refresh again.
