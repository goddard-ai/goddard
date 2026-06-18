# Workforce

> Workforce is Goddard's daemon-owned coordination model for multiple agents working in one repository. This page explains the runtime, queue, ownership, validation, and recovery rules at a conceptual level.

- **Core idea**
  - Workforce is daemon-owned multi-agent delegation for one repository workspace.
  - The daemon owns lifecycle, queue projection, validation, recovery, and event streaming.

- **Runtime lifecycle**
  - An operator explicitly starts workforce orchestration for a repository workspace.
  - Starting safely reuses an already-running runtime for the same repository instead of creating a duplicate.
  - The daemon reconstructs workforce state from durable repository-local intent before accepting new work.
  - The runtime can be inspected, listed, shut down, and streamed through daemon clients.

- **Initialization and discovery**
  - Candidate discovery helps an operator choose package or domain boundaries for workforce setup.
  - Initialization creates repository workforce intent and ledger state through the daemon.
  - Initialization is a repository workflow action, not an agent session by itself.

- **Handling**
  - Each handled request runs in a fresh agent session with workforce context.
  - Workforce sessions may share one repository working tree while still preserving per-agent ownership boundaries.
  - Agents can delegate additional work, respond to active work, or suspend work through daemon-injected tools.
  - The queue advances only after the daemon accepts the handling outcome for the active request.
  - A session failure, validation failure, or explicit suspension leaves the request visible for recovery instead of treating it as completed.

- **Boundaries**
  - Only one active workforce runtime may exist for a given repository workspace.
  - Workforce orchestration is separate from pull request feedback handling.
  - SDK and operational clients observe and request mutations; they do not own independent workforce runtime state.
