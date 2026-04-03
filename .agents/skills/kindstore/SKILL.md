---
name: kindstore
description: Model, query, and migrate kindstore-based document stores. Use when Codex needs to design kinds, choose tags and indexes, write collection queries, use findPage keyset pagination, store metadata, batch writes, or evolve schemas with payload or structural migrations while staying inside kindstore's intentionally narrow typed query model.
---

# Kindstore

## Workflow

Inspect the current repository code before editing. Confirm the existing kind declarations, query shapes, and migration patterns instead of assuming they match the examples here.

Keep solutions inside kindstore's narrow scope. It is a registry-driven document store, not a general ORM.

## Use Kindstore Correctly

Use `kind(tag, schema)` with stable tags.

Declare only fields that truly need typed filtering or ordering.

Use single-field indexes for simple query predicates. Add composite indexes only for real combined query shapes.

Use explicit SQLite type hints for numeric fields that need integer or numeric ordering semantics, especially timestamps.

Use store-managed timestamps only when the store should own those payload fields.

Use `put()` for replacement writes and `update()` for shallow or computed changes.

Use `first()`, `findMany()`, `findPage()`, and `iterate()` based on whether you need one row, an eager array, forward keyset pagination, or incremental processing.

Treat typed queries as limited to explicitly indexed top-level fields plus simple comparisons, ordering, and limits.

Prefer typed APIs for normal reads and writes. Use raw SQL only as an escape hatch.

For `findPage()`, require explicit `orderBy`, require a positive `limit`, keep the same `orderBy` when continuing with `after`, and avoid null ordered boundary values across pages.

Use metadata for small store-level values such as preferences, checkpoints, and sync cursors.

Use `batch()` when related writes must succeed or fail together.

Choose payload migration for document-shape changes within one kind. Choose structural migration for renames, drops, or tag changes.

Treat payload migrations as eager upgrades that run before normal typed reads and queries.

## Validation

Run the relevant project tests after changing code that uses kindstore.

Check the existing application behavior around ordering, pagination, and migrations rather than assuming a generic document-store pattern will fit.
