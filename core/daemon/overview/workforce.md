# Workforce

- **Core idea**
  - Workforce is daemon-owned multi-agent delegation for one repository workspace.
  - The daemon owns lifecycle, queue projection, validation, recovery, and event streaming.
  - Operators and agents interact through daemon controls instead of creating independent queue or watcher state.

- **Runtime lifecycle**
  - An operator explicitly starts workforce orchestration for a repository workspace.
  - Starting safely reuses an already-running runtime for the same repository instead of creating a duplicate.
  - The daemon reconstructs workforce state from durable repository-local intent before accepting new work.
  - The runtime can be inspected, listed, shut down, and streamed through daemon clients.

- **Initialization and discovery**
  - Candidate discovery helps an operator choose package or domain boundaries for workforce setup.
  - Initialization creates repository workforce intent and ledger state through the daemon.
  - Initialization is a repository workflow action, not an agent session by itself.

- **Requests**
  - A workforce request targets one workforce agent.
  - Requests for the same agent are handled sequentially.
  - A request can be created, updated, cancelled, truncated, responded to, or suspended through daemon controls.
  - Queue mutations return the updated workforce projection so clients can stay aligned with daemon truth.

- **Handling**
  - Each handled request runs in a fresh agent session with workforce context.
  - Workforce sessions may share one repository working tree while still preserving per-agent ownership boundaries.
  - Agents can delegate additional work, respond to active work, or suspend work through daemon-injected tools.

- **Validation**
  - A response is a validation gate, not a blind completion signal.
  - The daemon validates attributable git state and commits before advancing the queue.
  - Work that touches paths outside the responding agent's ownership can be suspended for human review.
  - Dirty tracked changes inside the responding agent's owned paths can block completion.

- **Suspension and recovery**
  - Suspended work remains blocked until explicit operator or root-agent action resolves it.
  - Individual agent-session failure should not corrupt the broader workforce queue.
  - Daemon restart should recover operator-visible workforce progress from durable state.
  - Shutdown stops new handling cleanly and preserves enough intent for later restart.

- **Boundaries**
  - Only one active workforce runtime may exist for a given repository workspace.
  - Workforce orchestration is separate from pull request feedback handling even when hosted by the same daemon process.
  - SDK and operational clients observe and request mutations; they do not own independent workforce runtime state.
