# ADR: Daemon Session Identity Freeze (`id` vs `acpId`)

## Status

Accepted

## Context

Plan 0 (`plan0_contract_identity_freeze.md`) requires a stable identity contract before storage migration, multi-session host work, and daemon control-plane wiring proceed in parallel.

Historically, code paths mixed multiple identifiers (`sessionId`, ACP session identifiers, and `serverId`-based discovery), which caused interface churn and cross-track merge conflicts.

## Decision

1. `sessions.id` is the daemon-owned internal identifier and the canonical primary key.
2. `sessions.acpId` is the ACP protocol-facing session identifier (unique) and only used for ACP transport mapping.
3. Daemon session HTTP/WS routes are keyed by internal `:id`:
   - `POST /sessions`
   - `GET /sessions/:id`
   - `GET /sessions/:id/history`
   - `POST /sessions/:id/shutdown`
   - `WS /sessions/:id/acp`
4. Runtime contracts use a well-known daemon socket path and session token authorization instead of injecting daemon URL/session ID env variables.
5. `serverId` is not required for routing/discovery contracts going forward.

## Consequences

- Parallel tracks can implement independently against a shared contract.
- Storage and runtime internals can evolve as long as external contracts preserve `id` as canonical and `acpId` as protocol mapping.
- Remaining `serverId` usages are treated as migration debt and should be removed in follow-up plans.
