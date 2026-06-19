# Session Lifecycle

> A daemon session is the local record and live process boundary for one agent conversation or task. This page explains creation, live control, completion, shutdown, and the transition to history-only inspection.

## Core idea

- A daemon session is one daemon-supervised agent conversation or task.
- The daemon gives the session stable identity, access control, status, history, diagnostics, and optional workspace isolation.
- Clients should treat the daemon's session record as the current truth for whether a session is live, completed, cancelled, failed, blocked, idle, or only inspectable as history.

## Creation

- A caller supplies the intended working directory and launch intent.
- Launch intent may include an agent, initial prompt, model choice, config choices, repository scope, pull-request scope, and worktree preference.
- The daemon resolves effective launch policy before starting the agent.
- If required launch policy cannot be resolved, the session should fail before users are asked to continue work in a half-started runtime.

## Session forms

- Interactive sessions can accept later prompts while live.
- One-shot sessions are intended to finish after their initial prompt.
- Action, loop, workforce, pull-request, and direct client flows can all create daemon sessions.

## Active sessions

- A live session can receive prompts, emit messages, update status, and be controlled by clients.
- The daemon serializes prompt delivery so one prompt turn is active at a time.
- Active sessions may have queued prompts waiting for their turn.
- Queued prompts remain daemon-owned work until they are delivered, aborted by cancellation, or superseded by a steering action.
- A client may reconnect to live state through the daemon, but it should not assume stored history alone means a process is still running.

## Completion and shutdown

- A session can finish its work normally, be cancelled, report a blocker, become idle, error, or be explicitly shut down.
- Completing a session's inbox concern is separate from shutting down the session.
- Shutdown ends live execution for one session; it does not delete the session's inspectable record.
- Cancellation, agent failure, daemon shutdown, and normal completion are different outcomes even when all of them end live interaction.
- The user-facing difference is whether more work can be sent to the session or whether the record is now only useful for inspection.

## Reconnect and history-only state

- A reconnectable session still has live execution available.
- A history-only session has stored records but no live execution to resume.
- After daemon restart, sessions whose live execution is gone become inspectable as historical records rather than pretending to still be live.
- Recovery after restart belongs to the daemon: clients should refresh session state instead of deciding from cached connection state.
