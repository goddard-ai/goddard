# Plan D — Integration + Cutover (Serial Finalization)

## Context / Why

### Problem statement
After parallel tracks land, behavior is still fragmented until wiring, lifecycle enforcement, and old-path removal are done together.

### Why this approach
A dedicated cutover phase reduces risk: we integrate once on top of `@goddard-ai/ipc` (`core/ipc`, `./server`), verify end-to-end guarantees, then remove dead bespoke server paths in a controlled step.

### Success condition
Daemon is the practical and architectural source of truth for multi-session ACP lifecycle and security cleanup.

### Non-goals
- No new `cmd/` features.
- No desktop `app/` UX work.
- No continued bespoke HTTP/WS daemon server ownership after cutover.

## Depends on
- Plan A, Plan B, Plan C merged

## Goal
Wire all tracks together, enforce lifecycle authority, and complete daemon-hosted session provisioning cutover.

## Scope
- daemon + core/session integration
- daemon wiring through `@goddard-ai/ipc/server` (`core/ipc`)
- session lifecycle security hooks
- client/bootstrap path cutover (non-`cmd`)

## Implementation
1. Integrate daemon endpoints with real `SessionHost` implementation through `@goddard-ai/ipc` `./server` (`core/ipc`).
2. At session creation:
   - mint token
   - persist permissions by internal session ID
   - inject `GODDARD_SESSION_ID` + session token into agent env
3. At child exit/shutdown:
   - revoke permissions/token immediately
   - close session WS clients
   - remove runtime from registry
   - persist final status safely
4. Replace old standalone/bespoke session-server bootstrap paths with the shared `@goddard-ai/ipc/server` path (or guard with explicit fallback flag during transition).
5. Remove dead code tied to `serverId` routing assumptions.
6. Remove dead bespoke HTTP/WS server scaffolding once fallback is retired.

## Tests
- Integration test: create two sessions, verify isolation and no cross-talk through `@goddard-ai/ipc/server`.
- Integration test: kill one session, verify only that session is revoked/cleaned.
- Integration test: session tool updates via `GODDARD_SESSION_ID` succeed.
- Full regression: daemon PR security tests still green on the shared IPC server.

## Exit criteria
- daemon is authoritative host for multi-session ACP in practice via `@goddard-ai/ipc/server`.
- no runtime dependency on `serverId` lookup remains.
- no bespoke daemon HTTP/WS session server path remains.
- lint/typecheck/tests pass across touched packages.
