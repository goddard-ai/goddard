# Task: 030-add-daemon-terminal-websocket-and-sdk-surface

## Status

planned

## Objective

Expose daemon-owned terminals through one stable connection-scoped transport that other hosts can reuse.

## Scope

- Implement the daemon websocket endpoint.
- Add connection and terminal instance bookkeeping.
- Add output event publishing and disconnect teardown.
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

## Work-Ahead Safety

One task ahead is safe only for Bun-host bridge scaffolding that consumes the approved SDK/client surface without inventing new protocol semantics.

