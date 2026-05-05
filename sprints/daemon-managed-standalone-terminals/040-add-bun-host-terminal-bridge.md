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

- Review question: Does the Bun host own the daemon terminal connection and expose only the intended browser-safe terminal bridge to the app webview?
- Approval means: App state can treat terminals as Bun-host-backed resources and does not need direct daemon access or app-only protocol semantics.
- Downstream unlock: `050` can build user-visible standalone terminal tabs on the approved host bridge.
- Rework trigger: If ownership moves into the webview, lifecycle cleanup is ambiguous, or bridge payloads diverge from the shared terminal contract, the app surface must be revised.
- Revert or revision boundary: This bridge can be revised without discarding daemon terminal runtime and SDK work, but app tab work should wait for approval.

## Work-Ahead Safety

One task ahead is safe only for app-state scaffolding that consumes the approved host bridge shape. Avoid user-visible launch semantics before this task is reviewed.
