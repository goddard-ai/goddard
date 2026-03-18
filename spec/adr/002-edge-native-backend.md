# ADR-002: Edge-Native Backend (Cloudflare Workers + Durable Objects)

## Status
ACTIVE

## Context

The Goddard backend must handle two distinct workloads:
1. **Stateless request handling** — auth, PR creation, webhook ingest. These are short-lived, independently scalable operations.
2. **Stateful real-time fan-out** — maintaining open SSE connections for authenticated users and broadcasting managed PR events to the correct owner. This requires stream state that survives across individual requests.

We needed a deployment model that serves both workloads with sub-second global latency, without managing servers.

## Decision

The backend runs on **Cloudflare Workers** for stateless request handling and **Cloudflare Durable Objects** for user-scoped SSE state and event fan-out. **Turso** (SQLite at the Edge) provides durable persistence for users, sessions, GitHub App installations, and managed PR ownership.

## Rationale

- **Global low latency:** Workers execute at the edge closest to the requester, satisfying the Real-Time pillar.
- **Durable Objects as stream anchors:** Each authenticated Goddard user can be mapped to a stream owner with isolated subscriber state and predictable fan-out behavior.
- **No server management:** Cloudflare handles scaling, availability, and distribution.
- **Turso complements the edge model:** SQLite at the edge avoids round-trips to a centralized database region.

## Consequences

- Local development uses an in-memory control plane as a substitute for Turso and Durable Objects — this is a development convenience only, not a production mode.
- Production deployment requires Cloudflare account setup, `wrangler.toml` configuration, and Turso credentials.
- All backend logic must be compatible with the Workers runtime and must route real-time delivery by managed PR ownership rather than repository subscription lists.
