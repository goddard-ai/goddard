# ADR-001: SDK-As-Daemon-Control-Plane

## Status
ACTIVE

## Context

Goddard has multiple local clients that need to authenticate through the daemon and control daemon-managed automation consistently without re-implementing daemon contracts in each host.

## Decision

`@goddard-ai/sdk` is the daemon control plane.

It owns the programmatic surface for daemon-backed authentication and daemon-managed local automation control. It does not own general backend event streaming or other non-daemon real-time delivery concerns.

## Rationale

- **Single control surface:** Hosts use one shared daemon-facing client contract instead of inventing parallel control APIs.
- **Thin consumers:** Desktop, CLI, and other local clients remain small because daemon control behavior is centralized.
- **Clear boundary:** Backend-owned real-time delivery stays separate from daemon-control concerns.

## Consequences

- New daemon control capabilities must be added to the SDK first, then surfaced through consumers.
- Consumers are intentionally kept small and logic-light on daemon-facing behavior.
- The SDK must maintain zero runtime environment assumptions (browser, Node, Cloudflare) to remain usable in all supported daemon-control contexts.
