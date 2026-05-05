# Task: 030-add-daemon-terminal-websocket-and-sdk-surface

## Status

planned

## Objective

Expose daemon-owned terminals through one stable request/stream surface that other hosts can reuse.

## Scope

- Implement daemon terminal HTTP/IPC request handlers.
- Add connection and terminal instance bookkeeping.
- Add daemon stream event publishing and disconnect teardown.
- Add the SDK or node-surface client that speaks the terminal protocol.

## Dependencies

- `020-validate-daemon-pty-runtime` must prove daemon PTY viability.

## Acceptance Criteria

- A single client connection can create and control multiple concurrent terminal instances by local instance id.
- Terminal commands are rejected for unknown or non-owned instances.
- Disconnecting the client disposes all terminal instances for that connection.
- The SDK-facing client surface can create terminals, send input, resize, restart, close, and subscribe to terminal events without app-specific logic.
- Automated coverage exists for multi-instance behavior and disconnect disposal.

## Review Checkpoint

The human is reviewing the public daemon terminal surface and ownership/security behavior.

## Review Report

- Review question: Does the daemon expose the right request methods, stream events, SDK surface, and ownership checks for host-managed terminals?
- Approval means: The Bun host can consume the daemon terminal surface without inventing protocol behavior or bypassing connection ownership rules.
- Downstream unlock: `040` can wire the Bun host to the daemon terminal client and route terminal events to the app webview.
- Rework trigger: Changes to request names, stream event payloads, connection teardown semantics, or SDK method shape would force bridge and app-state rework.
- Revert or revision boundary: This task can be revised without discarding app UI work if `040` has not yet treated the SDK surface as stable.

## Work-Ahead Safety

One task ahead is safe only for Bun-host bridge scaffolding that consumes the approved SDK/client surface without inventing new protocol semantics.
