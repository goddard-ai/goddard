# Task: 040-add-bun-host-terminal-bridge

## Status

planned

## Objective

Make the Bun host the only daemon terminal client and expose terminal control/events to the app webview through Electrobun.

## Scope

- Add Bun-host terminal connection management.
- Add terminal ownership bookkeeping for host and webview lifecycle.
- Add browser-safe RPC/messages for terminal create, write, resize, restart, and close.
- Deliver daemon terminal events from the Bun host into the app.

## Dependencies

- `030-add-daemon-terminal-websocket-and-sdk-surface` exposes a stable request/stream terminal client interface.

## Acceptance Criteria

- The webview does not connect to the daemon terminal stream directly.
- The Bun host can multiplex multiple daemon terminal instances and route them back to the webview with stable local ids.
- Webview reload and teardown cleanup is defined and implemented so the host does not leak app-side terminal registrations.
- The bridge API is narrow and aligned to the shared terminal contract rather than inventing app-only behavior.

## Review Checkpoint

The human is reviewing the host boundary and whether terminal ownership is correctly centered in the Bun host.

## Review Report

### Plain-English Summary

This task will make the Bun host the sole daemon terminal client for the app. The webview should receive a narrow, browser-safe terminal bridge over Electrobun instead of connecting directly to daemon terminal streams or daemon IPC.

### How To Verify Without Reading Code

After implementation, review the reported bridge and lifecycle checks. Acceptance should mean the Bun host can multiplex multiple daemon terminals, route events to the right webview terminal ids, and clean up app-side registrations on reload or teardown without exposing daemon access directly to the browser.

### Agent Verification

- Pending implementation. Replace this with the exact automated checks and app/host diagnostics run before marking the task finished-unreviewed.

### Approval Questions

- Does the Bun host own the daemon terminal connection rather than the webview?
- Is the browser-safe bridge narrow and aligned to the shared terminal contract?
- Are webview reload, teardown, and host cleanup semantics clear enough for app terminal state to depend on them?

### Known Limits

- This task should not implement the final user-facing terminal tab experience.
- App state work should wait for this bridge shape to be accepted before treating terminal instances as stable user-visible resources.

## Work-Ahead Safety

One task ahead is safe only for app-state scaffolding that consumes the approved host bridge shape. Avoid user-visible launch semantics before this task is reviewed.
