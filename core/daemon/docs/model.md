# Daemon Model

The daemon is the local process that owns the lifecycle of Goddard automation, including sessions, background runtimes, and local state. This page defines the main parts of that responsibility before the feature-specific pages describe individual workflows.

## Core idea

- The daemon is the local lifecycle authority for Goddard automation.
- It runs as a local background process and exposes a daemon server for approved local clients.
- Clients observe, start, and steer daemon-owned work; they do not create parallel ownership of mutable runtime state.

## Daemon process

- Owns long-running local runtime state.
- Starts background services and the local control surface requested at launch.
- Stops hosted runtimes when the daemon itself shuts down.
- Remains headless:
  - The app is the primary human-facing surface.
  - The SDK is the primary programmatic surface.
  - The daemon is the trusted local execution boundary behind those surfaces.

## Daemon server

- Exposes local control and observation to app, SDK, and operational tools.
- Provides health, session, inbox, pull request, review session, agent, auth, action, loop, and workforce capabilities in the default product surface.
- Streams live events for surfaces that need to observe changes without owning the runtime.

## Feature composition

- The daemon provides substrate behavior:
  - process lifetime
  - local server lifetime
  - logging and request context
  - persistence setup
  - root configuration loading and refresh
  - shared session launch policy
- Product capabilities are composed as daemon features.
- The default daemon product surface includes:
  - sessions
  - inbox
  - pull requests
  - review sessions
  - actions
  - loops
  - workforce
  - agents
  - auth
- Feature packages own their product behavior while the daemon keeps shared runtime boundaries coherent.

## Runtime domains

- A runtime domain is a daemon-owned area of local execution, such as:
  - an interactive or one-shot agent session
  - a named action session
  - a loop runtime
  - pull request feedback handling
  - review-session synchronization around an isolated worktree
  - workforce orchestration
- Runtime domains may share daemon infrastructure.
- They must not share mutable execution state in ways that blur ownership.

## Live and historical state

- Live runtime state exists only while the daemon process owns active execution.
- Durable records keep important facts inspectable after work ends or after a daemon restart.
- A session can be live and reconnectable, or history-only after live execution is gone.
- Local daemon data belongs to the active data profile and is not automatically synced across machines.

## Recovery

- Daemon restart breaks live process ownership, but stored records remain.
- Startup reconciliation updates persisted session truth so clients can distinguish live reconnectable work from history-only records.
- Workforce runtimes rebuild their operator-visible queue state from durable workforce intent before accepting new work.
- Invalid persisted configuration does not replace the last valid daemon behavior until corrected.

## Guardrails

- The daemon is the sole lifecycle authority for daemon-owned runtimes.
- Approved clients can request mutations only through daemon contracts.
- Repository-local configuration may declare non-executable intent, but repository-local config cannot silently add trusted executable daemon extensions.
- User-scoped executable extensions are a separate trust boundary.
