---
id: adr-001-sdk-first-architecture
status: ACTIVE
links:
  - type: Relates-To
    target: spec/architecture.md
  - type: Relates-To
    target: spec/non-goals.md
---

# ADR-001: SDK-First Architecture

## Status
ACTIVE

## Context

Goddard has multiple runtime consumers: an interactive CLI for human developers, a feedback daemon, and an autonomous agent loop for operators. These consumers need shared platform capabilities: PR creation, webhook event streaming, and GitHub identity management.

The question was how to share those capabilities without duplicating logic across consumers or coupling them to each other.

## Decision

All platform capabilities are implemented once in `@goddard-ai/sdk`. The CLI (`@goddard-ai/cmd`) and the agent loop are thin consumers of the SDK — they add terminal UX and orchestration behavior, not platform logic.

No capability that belongs in the SDK may be re-implemented in a consumer package.

## Rationale

- **Single source of truth:** Bug fixes, API changes, and new capabilities propagate to all consumers automatically.
- **Testability:** Platform logic can be tested in isolation without spinning up CLI processes.
- **Third-party extensibility:** External integrations can consume `@goddard-ai/sdk` directly without depending on CLI internals.
- **Enforces the thin control-plane boundary** described in [`../non-goals.md`](../non-goals.md).

## Consequences

- New platform features must be added to the SDK first, then surfaced through consumers.
- Consumers are intentionally kept small and logic-light.
- The SDK must maintain zero runtime environment assumptions (browser, Node, Cloudflare) to remain usable in all consumer contexts.
