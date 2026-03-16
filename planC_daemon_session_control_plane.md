# Plan C — Daemon Session Control Plane + WS Multiplexing (Parallel Track)

## Context / Why

### Problem statement
Daemon currently secures PR operations but does not yet expose first-class session control and ACP websocket multiplexing for multiple sessions.

### Why this approach
Adding daemon-owned session endpoints and WS routing via `@goddard-ai/ipc` (`core/ipc`, `./server`) makes the daemon the authoritative control plane at the well-known socket path without maintaining a bespoke HTTP/WS stack.

### Success condition
Daemon can create/manage multiple session channels over one UDS listener while retaining existing PR security behavior.

### Non-goals
- No deep `core/session` refactor in this plan.
- No storage identity migration internals in this plan.
- No new bespoke HTTP/WS server code paths outside `@goddard-ai/ipc` `./server`.

## Depends on
- `plan0_contract_identity_freeze.md` merged

## Goal
Add daemon-hosted session control endpoints and ACP WS multiplex routing on the well-known daemon socket.

## Scope
- daemon IPC wiring to `@goddard-ai/ipc/server`
- `core/ipc` server entrypoint usage for daemon routes
- daemon startup wiring
- daemon tests

## Implementation
1. Boot one long-lived session host dependency at daemon startup (injection-friendly).
2. Replace any bespoke daemon HTTP/WS server construction with `@goddard-ai/ipc` `./server` from `core/ipc`.
3. Register daemon IPC endpoints through that server:
   - `POST /sessions`
   - `GET /sessions/:id`
   - `GET /sessions/:id/history`
   - `POST /sessions/:id/shutdown`
4. Register WS upgrade routing for `/sessions/:id/acp` through the same `@goddard-ai/ipc/server` instance.
5. Keep existing PR-security endpoints (`/pr/submit`, `/pr/reply`) intact on the same server.
6. Route identities exclusively by internal session `id`.

## Tests
- Endpoint contract tests for create/get/history/shutdown via `@goddard-ai/ipc/server`.
- WS attach tests for session path routing.
- Negative tests: unknown session ID, malformed route, unauthorized access where required.
- Regression tests confirming PR security routes still pass on the shared IPC server.

## Out of scope
- storage schema migration internals
- core/session deep refactor details
- SDK/client cutover

## Exit criteria
- daemon serves session control + PR security on same UDS listener via `@goddard-ai/ipc/server`.
- WS multiplexing by session ID works with test coverage.
- no bespoke daemon HTTP/WS server path remains for these routes.
