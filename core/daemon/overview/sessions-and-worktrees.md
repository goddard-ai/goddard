# Sessions and Worktrees

- **Core idea**
  - A daemon session is one daemon-supervised agent conversation or task.
  - The daemon gives the session stable identity, access control, status, history, diagnostics, and optional workspace isolation.
  - Sessions may be interactive, one-shot, action-created, loop-created, workforce-created, pull-request-related, or directly created by a client.

- **Session creation**
  - A caller provides the intended working directory, agent choice or defaults, optional initial prompt, optional model and config choices, and optional repository or pull-request scope.
  - The daemon resolves the effective agent and launch policy before starting the agent.
  - A one-shot session is intended to finish after its initial prompt.
  - A normal interactive session remains available for later prompts until it ends or is shut down.

- **Launch previews and leases**
  - A launch preview lets a client discover adapter and repository capabilities before committing to a durable session.
  - A launch lease can keep a prepared live ACP session available while a launch dialog is still being completed.
  - If the launch is abandoned, the lease can be released without creating a durable daemon session.

- **Session identity and access**
  - The daemon session id is the stable local identity for session operations.
  - The ACP session id belongs to the agent protocol boundary.
  - A session token grants narrow authority for tools running inside a daemon-launched session.
  - Token-based commands can resolve their current daemon session without broad daemon access.

- **Live control**
  - Clients can send prompts to a live session.
  - The daemon serializes queued prompts so the agent receives one active prompt turn at a time.
  - Clients can cancel active work and inspect queued prompts the daemon aborted instead of replaying silently.
  - Clients can steer active work by cancelling and injecting a replacement prompt after a safe boundary.
  - Clients can update supported model and agent config options on active sessions.
  - Shutdown asks one session to stop and reports whether the daemon observed the stop.

- **History and diagnostics**
  - Session history is the conversation and agent protocol record clients use to reconstruct what happened.
  - Live message streams let clients observe new session messages in real time.
  - Lifecycle streams let clients observe app-wide session state changes without subscribing to transcript content.
  - Diagnostics are structured lifecycle facts such as creation, status changes, reconciliation, shutdown, and failures.
  - History and diagnostics remain useful after live execution is gone.

- **Connection modes**
  - A live reconnectable session can accept continued interaction.
  - A history-only session can be inspected but not resumed as the same live process.
  - Daemon restart can turn previously live sessions into history-only records when the underlying live execution no longer exists.

- **Composer and repository discovery**
  - Session-scoped composer suggestions reflect available commands and session context.
  - Draft composer suggestions can be requested before a session exists when only a repository working directory is known.
  - Subpackage discovery helps clients choose launchable working directories inside a project.

- **Session worktrees**
  - A session may opt into an isolated linked Git worktree.
  - The worktree gives daemon-managed work its own checkout instead of mutating the caller's primary checkout.
  - The effective working directory can remain a subdirectory inside the isolated repository worktree.
  - Persisted worktree metadata lets clients inspect the workspace attached to a session after launch.
  - Worktree cleanup is explicit; session exit or daemon restart does not automatically remove isolated worktrees.

- **Fresh worktree preparation**
  - Fresh default-plugin session worktrees can be prepared before the agent starts.
  - Preparation may reuse configured untracked artifacts from the source checkout when the fresh worktree starts from the same commit.
  - Preparation may run a daemon-owned package-manager bootstrap when repository intent or unambiguous inference supports it.
  - Artifact reuse is best-effort.
  - If the daemon decides a bootstrap command applies and that bootstrap fails, session launch fails instead of starting from a partially prepared checkout.
  - Repository-local configuration can shape non-executable preparation policy.
  - Custom worktree plugins are user-scoped executable extensions and are not declared by repository-local config.

- **Completion and attention**
  - Completing a session inbox concern is separate from shutting down the session.
  - A session can report an initiative, blocker, or turn-ended update through daemon-owned tools.
  - Those reports may refresh the daemon-local inbox row for the session.
