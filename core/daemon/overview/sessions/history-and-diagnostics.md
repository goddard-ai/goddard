# History and Diagnostics

> Session history records what happened in the agent conversation, while diagnostics record lifecycle facts about the daemon-managed run. This page explains what remains inspectable while a session is live and after it becomes history-only.

- **Session history**
  - History is the ordered conversation and agent protocol record associated with a session.
  - Clients use history to reconstruct what happened in a session.
  - History remains useful after live execution ends.

- **Live message streams**
  - A session message stream lets clients observe new transcript-related session events for one live session.
  - Stream consumers observe daemon-published session messages; they do not own prompt delivery.

- **Lifecycle streams**
  - Lifecycle streams expose app-wide session state changes.
  - They let clients update session lists or badges without subscribing to transcript content.

- **Diagnostics**
  - Diagnostics are structured lifecycle facts rather than the conversation itself.
  - They help operators and clients understand creation, launch, status changes, shutdown, reconciliation, and failures.

- **Restart behavior**
  - Daemon restart can end live execution.
  - Stored history and diagnostics remain the basis for later inspection.
  - Reconciliation updates connection truth so clients can distinguish reconnectable sessions from history-only records.
