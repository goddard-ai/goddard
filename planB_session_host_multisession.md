# Plan B — `core/session` Multi-session Host Extraction (Parallel Track)

## Context / Why

### Problem statement
`core/session` is still shaped around single-session server assumptions. That prevents daemon from centrally hosting and managing multiple ACP sessions.

### Why this approach
Extracting a reusable `SessionHost` isolates runtime/session orchestration from transport/control-plane details and unlocks multiplexed daemon hosting.

### Success condition
`core/session` can run multiple independent ACP runtimes concurrently in one process with correct isolation and lifecycle behavior.

### Non-goals
- No daemon IPC route implementation in this plan.
- No storage schema migration in this plan.
- No bespoke HTTP/WS server implementation in `core/session`; transport remains delegated to `@goddard-ai/ipc` (`core/ipc`, `./server`).

## Depends on
- `plan0_contract_identity_freeze.md` merged

## Goal
Refactor `core/session` from single-session server assumptions into a reusable in-process `SessionHost` handling multiple concurrent ACP sessions.

## Scope
- `core/session`

## Implementation
1. Introduce `SessionHost` abstraction:
   - `createSession(...)`
   - `attachWebSocket(id, ws)`
   - `getSessionHistory(id)`
   - `shutdownSession(id)`
   - `closeAll()`
2. Session runtime registry keyed by internal `id`.
3. Track ACP identity per runtime (`acpId`) for protocol calls.
4. Preserve existing behavior:
   - prompt injection (foreground/background)
   - status transitions (`active/done/cancelled`)
   - history capture
5. Ensure shutdown idempotency and per-session isolation.
6. Keep `SessionHost` transport-agnostic so daemon can wire it through `@goddard-ai/ipc` `./server` without a bespoke HTTP/WS stack.

## Tests
- Existing session tests remain green.
- New tests:
  - concurrent session isolation
  - no cross-session WS message leakage
  - targeted shutdown does not affect other sessions
  - child exit cleanup for one session only

## Out of scope
- Daemon routing/endpoints
- SDK/client cutover

## Exit criteria
- `SessionHost` is usable programmatically with multi-session behavior validated by tests.
