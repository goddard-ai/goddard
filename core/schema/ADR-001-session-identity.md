# ADR 001: Session Identity

## Context / Why

Today, session identity and routing assumptions are mixed (internal app identity vs ACP identity vs old server ID assumptions). That ambiguity causes churn and merge conflicts when multiple teams touch daemon/session/storage simultaneously.

We are freezing the core contracts first so downstream work can execute in parallel without reworking each other’s interfaces.

## Decisions

The following identity mapping rules are explicitly locked and codified:

1. `sessions.id` is the daemon-owned internal session ID (primary key).
2. `sessions.acpId` is the ACP protocol session ID (unique, protocol-facing).
3. The runtime environment uses `GODDARD_SESSION_ID` (not `GODDARD_SERVER_ID`).
4. Daemon session APIs are keyed by internal `:id` (e.g., `/sessions/:id`).
5. `serverId` is removed from required runtime routing/discovery contracts.
