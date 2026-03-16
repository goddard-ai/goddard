# Plan C — Daemon Session Control Plane + WS Multiplexing (Parallel Track)

## Context / Why

### Problem statement
Daemon currently secures PR operations but does not yet expose first-class session control and ACP websocket multiplexing for multiple sessions.

### Why this approach
Adding daemon-owned session endpoints and WS routing makes the daemon the authoritative control plane at the well-known socket path.

### Success condition
Daemon can create/manage multiple session channels over one UDS listener while retaining existing PR security behavior.

### Non-goals
- No deep `core/session` refactor in this plan.
- No storage identity migration internals in this plan.

## Depends on
- `plan0_contract_identity_freeze.md` merged

## Goal
Add daemon-hosted session control endpoints and ACP WS multiplex routing on the well-known daemon socket.

## Scope
- `daemon/src/ipc.ts`
- daemon startup wiring
- daemon tests

## Implementation
1. Boot one long-lived session host dependency at daemon startup (injection-friendly).
2. Add daemon IPC endpoints:
   - `POST /sessions`
   - `GET /sessions/:id`
   - `GET /sessions/:id/history`
   - `POST /sessions/:id/shutdown`
3. Add WS upgrade routing for `/sessions/:id/acp`.
4. Keep existing PR-security endpoints (`/pr/submit`, `/pr/reply`) intact.
5. Route identities exclusively by internal session `id`.

## Tests
- Endpoint contract tests for create/get/history/shutdown.
- WS attach tests for session path routing.
- Negative tests: unknown session ID, malformed route, unauthorized access where required.
- Regression tests confirming PR security routes still pass.

## Out of scope
- storage schema migration internals
- core/session deep refactor details
- SDK/client cutover

## Exit criteria
- daemon serves session control + PR security on same UDS listener.
- WS multiplexing by session ID works with test coverage.
