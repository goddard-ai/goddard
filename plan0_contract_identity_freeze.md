# Plan 0 — Contract + Identity Freeze (Serial Prerequisite)

## Context / Why

### Problem statement
Today, session identity and routing assumptions are mixed (internal app identity vs ACP identity vs old server ID assumptions). That ambiguity causes churn and merge conflicts when multiple teams touch daemon/session/storage simultaneously.

### Why this approach
We freeze the core contracts first so Plans A/B/C can execute in parallel without reworking each other’s interfaces.

### Success condition
All teams share one unambiguous identity model and one daemon session API shape before implementation begins.

### Non-goals
- No runtime behavior changes yet.
- No daemon/session host implementation yet.

## Goal
Freeze interfaces so downstream work can run in parallel without churn.

## Why this is first
All parallel tracks depend on stable identity semantics and daemon session API contracts.

## Decisions to lock
1. `sessions.id` is daemon-owned internal session ID (primary key).
2. `sessions.acpId` is ACP protocol session ID (unique, protocol-facing).
3. Runtime env uses `GODDARD_SESSION_ID` (not `GODDARD_SERVER_ID`).
4. Daemon session APIs are keyed by internal `:id`.
5. `serverId` removed from required runtime routing/discovery contracts.

## Deliverables
- Typed schema updates in `core/schema` for daemon session endpoints:
  - `POST /sessions`
  - `GET /sessions/:id`
  - `GET /sessions/:id/history`
  - `POST /sessions/:id/shutdown`
  - `WS /sessions/:id/acp`
- ADR-style note (or short design doc) codifying id/acpId mapping rules.
- Test stubs (failing or TODO) asserting internal-id keyed API behavior.

## Out of scope
- No runtime implementation.
- No `cmd/` changes.
- No desktop `app/` changes.

## Exit criteria
- Team agrees contracts are frozen.
- No remaining open questions around ID usage in routes/env/storage APIs.
