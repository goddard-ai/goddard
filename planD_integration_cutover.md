# Plan D — Integration + Cutover (Serial Finalization)

## Context / Why

### Problem statement
After parallel tracks land, behavior is still fragmented until wiring, lifecycle enforcement, and old-path removal are done together.

### Why this approach
A dedicated cutover phase reduces risk: we integrate once, verify end-to-end guarantees, then remove dead paths in a controlled step.

### Success condition
Daemon is the practical and architectural source of truth for multi-session ACP lifecycle and security cleanup.

### Non-goals
- No new `cmd/` features.
- No desktop `app/` UX work.

## Depends on
- Plan A, Plan B, Plan C merged

## Goal
Wire all tracks together, enforce lifecycle authority, and complete daemon-hosted session provisioning cutover.

## Scope
- daemon + core/session integration
- session lifecycle security hooks
- client/bootstrap path cutover (non-`cmd`)

## Implementation
1. Integrate daemon endpoints with real `SessionHost` implementation.
2. At session creation:
   - mint token
   - persist permissions by internal session ID
   - inject `GODDARD_SESSION_ID` + session token into agent env
3. At child exit/shutdown:
   - revoke permissions/token immediately
   - close session WS clients
   - remove runtime from registry
   - persist final status safely
4. Replace old standalone session-server bootstrap path (or guard with explicit fallback flag during transition).
5. Remove dead code tied to `serverId` routing assumptions.

## Tests
- Integration test: create two sessions, verify isolation and no cross-talk.
- Integration test: kill one session, verify only that session is revoked/cleaned.
- Integration test: session tool updates via `GODDARD_SESSION_ID` succeed.
- Full regression: daemon PR security tests still green.

## Exit criteria
- daemon is authoritative host for multi-session ACP in practice.
- no runtime dependency on `serverId` lookup remains.
- lint/typecheck/tests pass across touched packages.
