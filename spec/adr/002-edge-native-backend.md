---
id: adr-002-edge-native-backend
status: ACTIVE
links:
  - type: Relates-To
    target: spec/architecture.md
---

# ADR-002: Edge-Native Backend (Cloudflare Workers + Durable Objects)

## Status
ACTIVE

## Context

The Goddard backend must handle two distinct workloads:
1. **Stateless request handling** — auth, PR creation, webhook ingest. These are short-lived, independently scalable operations.
2. **Stateful real-time fan-out** — maintaining open SSE connections per repository and broadcasting events to all subscribers. This requires per-repository state that survives across individual requests.

We needed a deployment model that serves both workloads with sub-second global latency, without managing servers.

## Decision

The backend runs on **Cloudflare Workers** (stateless request handlers) with **Cloudflare Durable Objects** for per-repository SSE state and event fan-out. **Turso** (SQLite at the Edge) provides durable persistence for users, sessions, and GitHub App installations.

## Rationale

- **Global low latency:** Workers execute at the edge closest to the requester, satisfying the Real-Time pillar from [`../vision.md`](../vision.md).
- **Durable Objects as session anchors:** Each `owner/repo` maps to a Durable Object instance that owns all SSE connections for that repo, providing strong isolation with no cross-repo interference.
- **No server management:** Cloudflare handles scaling, availability, and distribution.
- **Turso complements the edge model:** SQLite at the edge avoids round-trips to a centralized database region.

## Consequences

- Local development uses an in-memory control plane as a substitute for Turso and Durable Objects — this is a development convenience only, not a production mode.
- Production deployment requires Cloudflare account setup, `wrangler.toml` configuration, and Turso credentials.
- All backend logic must be compatible with the Workers runtime (no Node.js built-ins).
