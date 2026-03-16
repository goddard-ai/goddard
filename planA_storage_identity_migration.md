# Plan A — Storage + Identity Migration (Parallel Track)

## Context / Why

### Problem statement
Current persistence semantics are ACP-coupled and still carry server-ID-era assumptions. That blocks clean daemon-owned lifecycle control and makes session lookup fragile.

### Why this approach
We separate concerns at the data layer: internal ID for product/runtime ownership, ACP ID for protocol mapping only.

### Success condition
All storage and lookup paths use internal `sessions.id` as canonical identity, with `acpId` used only where ACP transport requires it.

### Non-goals
- No daemon WS routing changes here.
- No multi-session host extraction here.
- No bespoke HTTP/WS server work; transport changes standardize on `@goddard-ai/ipc` (`core/ipc`, `./server`) in downstream plans.

## Depends on
- `plan0_contract_identity_freeze.md` merged

## Goal
Migrate persistence/API shape from ACP-coupled IDs to internal session IDs with ACP mapping.

## Scope
- `core/storage`
- `core/schema` (DB typing updates only as needed)
- Callers that resolve session records by server ID/env

## Implementation
1. Schema migration:
   - Keep `sessions.id` as PK (now internal UUID semantics).
   - Add `sessions.acpId` (unique; nullable during bootstrap if needed, then set).
2. Storage API updates:
   - Keep `SessionStorage.get(id)` (canonical internal lookup).
   - Add `SessionStorage.getByAcpId(acpId)`.
   - Remove `SessionStorage.getByServerId` once no callers remain.
3. Tool/runtime env resolution:
   - Move to `GODDARD_SESSION_ID` direct lookup where applicable.
4. Migration safety:
   - Provide deterministic backfill behavior for existing rows.

## Tests
- Unit tests for create/get/update using internal `id`.
- Unit tests for ACP mapping (`getByAcpId`).
- Regression test proving no serverId dependency remains.

## Risks
- Partial migration can strand callers on old ID assumptions.

## Exit criteria
- All touched paths compile without `getByServerId`.
- Storage tests pass with id/acpId semantics.
