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

### Plain-English Summary

This task will expose daemon-owned terminals through the approved HTTP request and daemon stream contract. It is responsible for multi-instance ownership checks, connection teardown disposal, and the SDK/client surface that hosts use to control terminals.

### How To Verify Without Reading Code

After implementation, review the reported request, stream, SDK, and ownership validation results. Acceptance should mean one host connection can create multiple terminals, cannot control terminals it does not own, receives terminal events through the stream surface, and disposes all owned terminals on disconnect.

### Agent Verification

- Pending implementation. Replace this with the exact automated checks and manual diagnostics run before marking the task finished-unreviewed.

### Approval Questions

- Does the daemon expose the right terminal request methods, stream events, and SDK/client surface for host-managed terminals?
- Are ownership checks strict enough to prevent a client from controlling terminals it did not create?
- Is disconnect teardown deterministic enough for the Bun host and app to rely on it?

### Known Limits

- This task should not implement Bun-host Electrobun bridging or app UI behavior.
- The task name still contains the earlier websocket wording, but the intended contract is HTTP requests plus daemon streams.

## Work-Ahead Safety

One task ahead is safe only for Bun-host bridge scaffolding that consumes the approved SDK/client surface without inventing new protocol semantics.
