# ADR-005: Daemon-Owned Workforce Runtime

## Status
ACTIVE

## Context

Workforce orchestration needs repository-scoped delegation, operator visibility, and reliable recovery after local interruption or restart. The open question was whether workforce runtime ownership should live in client layers or in the daemon.

The earlier client-owned direction favored watcher-style coordination and long-lived agent ownership within local tools. That approach split lifecycle authority across clients and made recovery depend too heavily on whichever process happened to be supervising the work.

## Decision

Workforce runtime ownership lives in the daemon.

The daemon owns repository-scoped workforce lifecycle, durable state reconstruction, and routing. SDK consumers and approved operational CLI tools act as thin control and observation clients rather than runtime owners. Each handled workforce request runs in a fresh agent session instead of relying on one long-lived workforce session.

## Rationale

- **Single lifecycle authority:** One daemon-owned runtime per repository keeps start, stop, recovery, and shutdown behavior consistent.
- **Recoverability:** Durable repository-local intent allows the daemon to reconstruct current workforce state after restart.
- **Shared control surfaces:** Desktop, SDK, and approved CLI clients can observe and control the same runtime without competing ownership.
- **Isolation by request:** Fresh sessions reduce the chance that stale in-memory context from unrelated work leaks across delegated tasks.
- **Clearer runtime boundaries:** Workforce orchestration remains distinct from other daemon-managed automation domains such as pull request feedback handling.

## Consequences

- Workforce control now depends on daemon availability rather than on a self-owning client process.
- Operator and client tooling must treat the daemon as the source of truth for workforce status.
- Delegated work no longer relies on a single long-lived workforce session for continuity.
- A narrow operational CLI is acceptable so long as it remains a thin daemon-backed control surface.

## Trade-off Acknowledged

A client-owned watcher model could preserve more warm in-memory context and may reduce per-request startup cost. It was rejected because it weakens recovery, splits runtime authority, and makes shared operator visibility harder to maintain.
