# Track B: Public Clients and Daemon

## Summary

This track removes repo-scoped stream subscription from the public clients and updates the daemon to consume one unified stream for the authenticated user.

## Implementation Changes

### Backend Client

- Replace `stream.subscribeToRepo(repo)` with `stream.subscribe()`.
- Open `/stream` without repo query parameters.
- Keep authentication and SSE parsing behavior unchanged.
- Remove repo-scoped stream helpers and tests rather than leaving dead compatibility code.

### SDK

- Replace `stream.subscribeToRepo(repo)` with `stream.subscribe()`.
- Keep the SDK stream payload contract unchanged.
- Update tests so malformed SSE handling and event dispatch are verified through the unified stream path.

### Daemon

- Stop splitting `input.repo` for stream subscription setup.
- Subscribe once at startup using the authenticated client.
- Continue using `event.owner`, `event.repo`, and `event.prNumber` from incoming payloads for logging, `isManaged` checks, and one-shot execution.
- Keep per-PR coalescing behavior unchanged.
- Remove repo-scoped subscription assumptions from tests.

## Public Interfaces

- `@goddard-ai/backend/client`: `stream.subscribe()`.
- `@goddard-ai/sdk`: `stream.subscribe()`.
- Daemon runtime behavior: one stream connection per process, many repos per stream.

## Test Plan

- Backend client tests verify unified `subscribe()` opens `/stream` and receives normal events.
- SDK tests verify unified `subscribe()` behavior and malformed SSE error handling.
- Daemon tests verify startup performs one subscription and still launches one-shots with correct repo and PR context from incoming events.
- Remove repo-specific subscription tests instead of preserving both code paths.

## Risks to Watch

- Leaving `subscribeToRepo(...)` in one client package but not the other will create drift between runtime surfaces.
- If daemon tests only cover one repo event, they can miss accidental filtering logic left over from the old model.
