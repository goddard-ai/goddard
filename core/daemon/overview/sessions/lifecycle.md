# Session Lifecycle

- **Core idea**
  - A daemon session is one daemon-supervised agent conversation or task.
  - The daemon gives the session stable identity, access control, status, history, diagnostics, and optional workspace isolation.

- **Creation**
  - A caller supplies the intended working directory and launch intent.
  - Launch intent may include an agent, initial prompt, model choice, config choices, repository scope, pull-request scope, and worktree preference.
  - The daemon resolves effective launch policy before starting the agent.

- **Session forms**
  - Interactive sessions can accept later prompts while live.
  - One-shot sessions are intended to finish after their initial prompt.
  - Action, loop, workforce, pull-request, and direct client flows can all create daemon sessions.

- **Active sessions**
  - A live session can receive prompts, emit messages, update status, and be controlled by clients.
  - The daemon serializes prompt delivery so one prompt turn is active at a time.
  - Active sessions may have queued prompts waiting for their turn.

- **Completion and shutdown**
  - A session can finish its work normally, be cancelled, report a blocker, become idle, error, or be explicitly shut down.
  - Completing a session's inbox concern is separate from shutting down the session.
  - Shutdown ends live execution for one session; it does not delete the session's inspectable record.

- **Reconnect and history-only state**
  - A reconnectable session still has live execution available.
  - A history-only session has stored records but no live execution to resume.
  - After daemon restart, sessions whose live execution is gone become inspectable as historical records rather than pretending to still be live.
