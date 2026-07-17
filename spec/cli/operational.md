# Daemon-Backed Operational CLI

The CLI initializes repository-local automation intent and controls daemon runtimes.

## Participants
- Operator managing local automation bring-up, inspection, or recovery
- Daemon runtime receiving lifecycle and mutation requests
- SDK-backed CLI implementation reusing shared contracts

## Capabilities
- Initialize repository-local automation state needed before runtime control begins.
- Start and stop supported runtimes.
- Inspect current runtime status.
- Submit or mutate operational commands that belong to automation domains.
- Expose an operator-facing daemon control shell for supported daemon capabilities when direct local inspection or recovery is needed.

## Boundaries
- The CLI must remain a thin operator surface over shared SDK and daemon contracts.
- Runtime lifecycle authority remains with the daemon, not with the CLI process.
- CLI behavior must stay operational rather than evolving into a broad interactive workspace.
- Any supported CLI capability must stay aligned with the same underlying capability in the SDK.
- A daemon control shell must derive its supported operations from the shared daemon control contract rather than maintaining a separate command inventory.
- A daemon control shell may expose low-level daemon capabilities for operators, but it must not turn those capabilities into new product workflows that bypass the app, SDK, or daemon ownership model.
- The CLI must not become the primary review, planning, or spec-authoring surface.
- This spec does not define command flags, output formatting, or shell ergonomics.
- The CLI must not serve as a workaround for capabilities that should instead live in the app or SDK.

## Rationale
A narrow CLI remains justified where operators need direct local automation control, but it must stay thin so the product does not reintroduce a parallel terminal-first surface.
