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

- `030-add-daemon-terminal-websocket-and-sdk-surface` exposes a stable terminal client interface.

## Acceptance Criteria

- The webview does not connect to the daemon terminal websocket directly.
- The Bun host can multiplex multiple daemon terminal instances and route them back to the webview with stable local ids.
- Webview reload and teardown cleanup is defined and implemented so the host does not leak app-side terminal registrations.
- The bridge API is narrow and aligned to the shared terminal contract rather than inventing app-only behavior.

## Review Checkpoint

The human is reviewing the host boundary and whether terminal ownership is correctly centered in the Bun host.

## Work-Ahead Safety

One task ahead is safe only for app-state scaffolding that consumes the approved host bridge shape. Avoid user-visible launch semantics before this task is reviewed.

