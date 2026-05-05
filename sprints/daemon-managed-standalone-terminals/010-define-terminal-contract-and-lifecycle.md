# Task: 010-define-terminal-contract-and-lifecycle

## Status

approved

## Objective

Lock the shared terminal protocol and lifecycle rules before runtime work starts.

## Scope

- Add shared schema/types for terminal HTTP request payloads, daemon stream events, and terminal runtime metadata.
- Define the SDK-facing terminal surface shape.
- Codify connection-scoped instance ids, Bun-host ownership, and disconnect disposal semantics.

## Dependencies

- Confirmed sprint architecture: Bun host owns the daemon terminal stream connection.
- Terminals are ephemeral to the Bun-host daemon terminal stream connection in this sprint.
- Human feedback replaced the prior websocket direction with HTTP requests plus daemon streams.

## Acceptance Criteria

- Shared types exist for terminal create, input, resize, restart, close requests and output, exit, title, cwd, error lifecycle events.
- The contract makes instance ids connection-local and forbids cross-client instance control by construction.
- The SDK surface shape is defined tightly enough that daemon, Bun host, and app can implement against one contract.
- The contract does not imply reconnect or resume across desktop-host disconnect in this sprint.

## Review Checkpoint

The human is reviewing the control contract and lifecycle semantics, not implementation detail.

## Review Report

- Review question: Does the terminal contract correctly establish daemon HTTP requests plus daemon streams as the shared control model, with connection-local instance ownership and disconnect disposal?
- Approval means: Later daemon runtime, Bun-host bridge, SDK, and app work can build against the request/stream contract without re-litigating transport semantics.
- Downstream unlock: `020` can validate daemon PTY ownership, and `030` can implement the daemon terminal request and stream surface.
- Rework trigger: Any change to terminal identity scope, reconnect/resume semantics, stream ownership, or SDK-facing method shape would force downstream transport and app bridge revisions.
- Revert or revision boundary: This task can be revised without discarding daemon PTY viability work as long as the core ownership invariant remains connection-local.

## Work-Ahead Safety

One task ahead is safe only for daemon PTY viability work, because that work depends on terminal ownership semantics but not on later UI decisions.

## Implementation Notes

- Added shared terminal HTTP request and daemon stream event schemas under `@goddard-ai/schema/daemon`.
- Defined daemon-minted terminal connection ids, connection-local terminal instance ids, create/input/resize/restart/close request payloads, and created/output/exit/title/cwd/error daemon events.
- Added SDK-facing terminal connection types that model request-based controls and stream-based events without implementing transport yet.
- Documented terminal terms in the schema and SDK glossaries.

## Feedback Notes

- Revised the task after human feedback to remove websocket terminology and model terminal control as HTTP requests plus daemon streams.

## Approval Notes

- Approved by the human after the request/stream contract revision.

## Verification Evidence

- `bun --cwd core/schema test`
- `bun --cwd core/sdk test`
- `bun run --cwd core/schema typecheck`
- `bun run --cwd core/sdk typecheck`
- `bun run --cwd core/schema lint`
- `bun run --cwd core/sdk lint`
