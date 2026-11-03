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

## Shared session transactions

Summon admission and Boss-chat creation use `StateStore::session_commit` to write
session rows and the corresponding Boss changes on the same SQLite connection.
The host retains the task-state lock through commit, then clears only the saved
batch's dirty marks. A failed admission removes its provisional in-memory shell;
SQLite rolls back the shell, employee, and new outcome together. Blank managed
sessions are durable even before a provider turn starts. Ordinary blank drafts
and incognito sessions retain their existing persistence policy.

Shared commits use FULL synchronization and restore the session connection's
prior policy on both success and rollback. Both stores publish their persistence
caches only after COMMIT. Boss notification occurs after the host releases its
task-state lock. The shared API is limited to changes that do not edit persona
text; generated persona files remain handled by normal Boss saves.

Provider dispatch, asynchronous transcript events, and report delivery remain
separate durable boundaries. In particular, an external provider acknowledgement
cannot join a local SQLite transaction; report/outbox replay identities still
protect those paths. Stable-runtime checkpoint writes retain their existing
transcript-plus-replay-cursor transaction.

## Contention measurements

`daemon-stats.jsonl` now includes a `persistence` array in its periodic samples.
No per-save file I/O is added. Each lane reports lifetime attempts/failures,
p95 over its last 256 attempts, and the latest sample. Phase fields use
microseconds: `prepare_us`, `writer_wait_us`, `transaction_us`, `commit_us`, and
`total_us`; samples also include `rows_changed` and `success`. Failed attempts
remain in the window. A phase that was not completed can have zero duration.
Preparation includes application mutex waits, serialization and diffing; the
writer wait specifically measures acquisition of an immediate SQLite transaction.
Total time includes setup and cleanup beyond the named phases.

Lanes are `sessions` (ordinary batch saves), `boss` (standalone Boss saves),
`sessions_boss` (the actual shared transaction), and `boss_shared_prepare` (the
Boss caller's preparation plus its host commit). The two shared lanes overlap;
do not add their durations or attempt counts. These measurements cover batch
writes and Boss saves, not every specialized database write in the process.

Boss diffs are prepared before acquiring the writer lock. A synthetic contention
probe uses a disposable database and three concurrent workers: ordinary session
saves, shared Boss/session saves, and standalone Boss saves:

```sh
goddard-agent resource run --json '{"resources":{"native_builds":1},"purpose":"persistence contention probe"}' -- mbx run -p waku-store --example persistence_contention -- 100
```

The JSON output includes elapsed time and the same phase metrics. This is a
repeatable diagnostic, not a performance gate or a model of a large live history.
Use live periodic samples to assess workload-specific contention before choosing
a separate database.
