# Boss persistence

The daemon opens Boss with `BossService::open_database(root, task_store.path())`.
Disabled services retain that database binding for later activation. Standalone
`BossService::open` and `disabled` retain the legacy JSON host for compatibility
and file-failure fixtures; daemon code must use the database constructors.

The schema is defined in `db/schema.ts` and migration 0033. Boss also embeds that
idempotent migration for standalone database hosts. `boss_coordinator` stores
schema version, an independent monotonically increasing storage revision, and
coordinator fields. `boss_records` stores one collection member per row, with
collection, parent identity, record identity, order, lifecycle, and typed JSON.
Nested chain steps, attempts, approvals and events, outcome assignments/handoffs,
and planning items/approved revisions have separate rows. Historical identities
have no cascading foreign key to sessions: deleting a transcript must preserve
assignment evidence. `boss_payloads` shares immutable requests, definitions,
results, persona text and approved content by SHA-256 digest.

An immediate SQLite transaction checks the previously loaded storage revision,
writes changed rows and payloads, removes obsolete rows and unreferenced payloads,
and publishes the coordinator. The in-memory state is published only after the
transaction commits. Boss connections use WAL and synchronous FULL. A conflicting
host write fails rather than replacing newer state; reload the service before
retrying. Schema version, record ordering, reachable records and payload digests
are validated on load. Invalid database data never triggers a JSON fallback.

If no database coordinator exists, the existing JSON loader reads `boss.json`
and its referenced retired history. The first successful save imports all records
in one transaction. JSON/history files remain untouched as migration backups and
are ignored after import. Editable plan and deliverable files stay on disk;
approval content remains exact captured bytes in the database. `PERSONA.md` files
remain generated projections of authoritative persona records.

The service still exposes a complete in-memory `BossState`, clones it for changes,
and scans/serializes records for comparison. This migration reduces disk writes;
it does not provide lazy history loading or incremental in-memory mutation.
Future changes to that interface must preserve atomic employee/outcome/report
publication and exact approval provenance.
