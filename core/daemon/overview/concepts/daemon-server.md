# Daemon Server

- **Core idea**
  - The daemon server is the local control surface exposed by the daemon process.
  - It lets approved local clients create, inspect, and mutate daemon-managed work without embedding daemon internals.

- **What it exposes**
  - Health checks for local availability.
  - Session creation, inspection, control, history, diagnostics, and streams.
  - Inbox listing, workflow updates, completion, and streams.
  - Pull request submission, lookup, and replies.
  - Review-session mount, run, and unmount behavior.
  - Adapter catalog management and auth session operations.
  - Action, loop, and workforce controls.

- **Streaming behavior**
  - Session message streams expose live transcript-related session events for one session.
  - Session lifecycle streams expose app-wide session state changes without transcript content.
  - Inbox and workforce streams expose daemon-published updates for clients that need live projections.

- **Boundaries**
  - The server is local control infrastructure, not a public external API contract.
  - The server does not make clients owners of runtime state.
  - Clients should treat daemon responses and streams as the current daemon truth for the active data profile.
