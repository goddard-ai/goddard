---
id: adr-004-sse-repo-stream
status: ACTIVE
links:
  - type: Relates-To
    target: spec/architecture.md
  - type: Relates-To
    target: spec/data-flows.md
---

# ADR-004: Repository Stream Transport Uses SSE

## Status
ACTIVE

## Context

The original real-time stream transport used WebSockets. In practice, the stream path is server-to-client only: webhook and PR events are published by the backend and consumed by CLI clients. There is no requirement for bidirectional messaging over the same connection.

Running interactive CLI consumers on Node also required extra WebSocket runtime handling and upgrade-specific server plumbing in local development.

## Decision

Repository event streaming uses **Server-Sent Events (SSE)** over standard HTTP (`text/event-stream`) instead of WebSocket upgrades.

## Rationale

- **Matches traffic shape:** The stream is one-way (server → client), which is SSE’s native model.
- **Simpler infrastructure path:** SSE removes explicit HTTP upgrade handling in local Node adapters.
- **Client portability:** SSE can be consumed with plain `fetch` stream parsing in the SDK, avoiding dependence on runtime WebSocket globals.
- **Durable Object compatibility:** Per-repo fan-out remains anchored in Durable Objects with the same isolation model.

## Consequences

- SDK stream subscriptions now open a long-lived HTTP request and parse SSE frames.
- Backend stream endpoints return `text/event-stream` responses and no longer rely on WebSocket `101` upgrades.
- Existing consumers that directly depended on WebSocket semantics must migrate to the SDK stream API or SSE parsing.
