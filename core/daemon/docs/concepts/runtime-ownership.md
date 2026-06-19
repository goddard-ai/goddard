# Runtime Ownership

The daemon is the source of truth for live automation state, while clients request changes and observe results through daemon contracts. This page explains the ownership boundary that keeps sessions and runtimes from splitting across clients.

## Core idea

- The daemon is the local lifecycle authority for daemon-managed automation.
- Clients can start, inspect, steer, and stop work through daemon contracts.
- Clients should not create a second source of truth for daemon-owned runtime state.

## What the daemon owns

- Session lifecycle and session reconciliation.
- Loop and workforce runtime lifecycle.
- Pull request feedback runtime handling.
- Inbox row creation and daemon attention refreshes.
- Root configuration loading, validation, watching, and last-good promotion.
- Optional session worktree provisioning and preparation.

## What clients own

- User intent and presentation.
- Programmatic calls through the SDK.
- User workflow choices such as reading, saving, archiving, or prioritizing inbox rows.
- Explicit requests to mutate daemon-owned state through supported daemon contracts.

## Why it matters

- Background automation remains recoverable after daemon restart.
- Multiple clients can observe the same work without racing each other.
- Guardrails such as session tokens, repository scope, and workforce ownership validation stay enforceable in one process boundary.
- Recovery is consistent because clients reload daemon-owned state instead of reconstructing it from partial local observations.

## Boundaries

- The app remains the primary human-facing workspace.
- The SDK remains the primary programmatic surface.
- The daemon remains the trusted local execution boundary.
- Runtime domains may share daemon infrastructure but must not share mutable execution state in a way that hides ownership.
