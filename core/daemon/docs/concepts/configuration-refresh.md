# Configuration Refresh

The daemon watches persisted Goddard configuration while it runs, but active work keeps the configuration it already resolved. This page explains when valid config changes affect future work and what happens when edits are invalid.

## Core idea

- The daemon owns persisted root-config loading and refresh for long-running local automation.
- Valid config changes affect future work after the daemon accepts the updated snapshot.
- Work that already started continues under the configuration resolved for that work.

## Last-good behavior

- The daemon preserves the last valid behavior when a persisted config edit is invalid.
- Invalid edits must be corrected before they affect future work.
- This avoids breaking active local automation because a watched config file is temporarily malformed.
- Clients should surface that future work is still using the last accepted config rather than pretending the invalid edit took effect.

## Configuration scopes

- User configuration supplies personal defaults and user-scoped trusted extension references.
- Repository configuration supplies shared repository intent.
- Runtime input supplies one-invocation overrides that do not implicitly become persisted preferences.
- The useful user question is which scope supplied the current effective behavior, not which file the daemon happened to read first.

## User configuration updates

- Authorized daemon clients can read the current user-scoped root document together with the composed JSON Schema for the running daemon build.
- Clients update one field at a time. The daemon applies each update to the latest document, validates the complete result, and atomically replaces the user config file while preserving unrelated fields.
- The daemon owns the persisted `$schema` marker. Optional values and schema defaults remain absent until a client explicitly updates them.
- Updates to daemon startup settings are saved but report that a daemon restart is required before they take effect.
- Repository configuration and one-invocation runtime overrides are not changed by user configuration updates.

## Trust boundary

- Repository-local configuration may shape non-executable daemon behavior such as session worktree preparation intent.
- Repository-local configuration cannot declare custom executable daemon worktree plugins.
- User-scoped executable extension settings are a separate trust boundary.

## User-facing outcomes

- Saving valid config can change how later sessions, actions, loops, or worktrees resolve.
- Existing work does not silently change configuration underneath an active run.
- If config watching degrades, the daemon should continue using accepted valid behavior until it can refresh again.
- Worktree preparation can react to accepted repository intent such as `.worktreeinclude`, while arbitrary repository shell hooks remain outside the trust boundary.
