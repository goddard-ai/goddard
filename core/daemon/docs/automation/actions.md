# Actions

A named action is reusable automation that starts a daemon-managed session for one focused run. This page explains what changes when an action is run and how action configuration feeds into the created session.

## Core idea

- An action is a reusable one-shot execution definition.
- Running a named action creates a daemon-managed session for that action.

## Resolution

- The daemon resolves the action's prompt and configuration from applicable configuration roots.
- Action-specific configuration inherits global and repository baseline configuration before action defaults apply.
- Runtime input remains ephemeral unless explicitly persisted elsewhere.

## Session behavior

- The resulting session can use ordinary session history, diagnostics, model and config options, inbox reporting, and optional worktree isolation.
- A named action is intended for one-shot work rather than a long-running runtime.
- Users should inspect the created session for output, errors, cancellation, blockers, or final status.
- If launch fails, the failure belongs to the action-created session workflow rather than to a separate action runtime.

## Boundaries

- Action defaults belong in machine-readable configuration, not prompt metadata.
- Running an action does not create a separate runtime ownership model outside sessions.
- Actions are related to [session lifecycle](../sessions/lifecycle.md), [configuration refresh](../concepts/configuration-refresh.md), and [worktrees](../sessions/worktrees.md) when isolation is requested.
