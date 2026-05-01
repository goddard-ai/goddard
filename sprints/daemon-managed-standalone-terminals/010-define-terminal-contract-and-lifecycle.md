# Task: 010-define-terminal-contract-and-lifecycle

## Status

planned

## Objective

Lock the shared terminal protocol and lifecycle rules before runtime work starts.

## Scope

- Add shared schema/types for terminal websocket frames and terminal runtime metadata.
- Define the SDK-facing terminal surface shape.
- Codify connection-scoped instance ids, Bun-host ownership, and disconnect disposal semantics.

## Dependencies

- Confirmed sprint architecture: Bun host owns the daemon terminal connection.
- Terminals are ephemeral to the Bun-host daemon websocket connection in this sprint.

## Acceptance Criteria

- Shared types exist for terminal create, input, resize, restart, close requests and output, exit, title, cwd, error lifecycle events.
- The contract makes instance ids connection-local and forbids cross-client instance control by construction.
- The SDK surface shape is defined tightly enough that daemon, Bun host, and app can implement against one contract.
- The contract does not imply reconnect or resume across desktop-host disconnect in this sprint.

## Review Checkpoint

The human is reviewing the control contract and lifecycle semantics, not implementation detail.

## Work-Ahead Safety

One task ahead is safe only for daemon PTY viability work, because that work depends on terminal ownership semantics but not on later UI decisions.

