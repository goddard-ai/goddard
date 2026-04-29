# Archived Session Worktree Snapshot and Restore

Status: proposed

## Overview

Daemon-managed sessions can already own isolated linked Git worktrees, but archiving a session only preserves history today. It does not define what should happen to the on-disk worktree when the user wants to reclaim disk without losing tracked changes.

This design adds one daemon-owned archive flow for session worktrees:

1. when the user archives a session, the daemon snapshots the worktree's tracked dirty state into a private Git ref,
2. the daemon cleans and removes the linked worktree from disk, and
3. when the user unarchives that session later, the daemon recreates the linked worktree and reapplies the snapshot.

The archive payload is Git-native. It uses a stash-style commit captured relative to the worktree `HEAD`, stored under a daemon-private ref namespace instead of the user's stash stack. Restore recreates the worktree in detached mode so the daemon does not create user-visible branches.

This design is scoped to daemon-managed linked worktrees. It does not restore live ACP connectivity or resume agent execution.

## Context

Today the daemon persists static worktree identity in `db.worktrees` and already uses Git-owned refs plus metadata files for mounted worktree sync state. The current worktree model has two relevant properties:

- `createWorktree()` already enforces that daemon-managed worktrees are real linked Git worktrees attached to the same common Git dir as the source repository.
- The default plugin creates a detached worktree first and then attaches a branch, so the daemon already depends on Git CLI behavior rather than custom repository metadata for the core lifecycle.

The archive feature should follow the same boundary:

- kindstore remains the source of truth for static session and worktree identity,
- Git owns the snapshot payload and its reachability,
- the daemon owns orchestration, cleanup, restore, and diagnostics.

## Goals

- Allow a user archive action to reclaim the on-disk session worktree without losing tracked modifications or staged state.
- Restore the archived worktree on user unarchive without creating a visible `refs/heads/*` branch.
- Keep the durable snapshot outside `refs/stash` so it is unaffected by user stash churn.
- Preserve enough metadata to restore even when the archived worktree was clean and therefore has no snapshot commit.
- Coordinate cleanly with existing daemon-managed worktree sync and session lifecycle behavior.
- Expose the archive state through `core/schema`, `core/daemon`, and `core/sdk`.

## Non-Goals

- Preserving untracked or ignored files in v1.
- Reconnecting or reviving live agent execution when a session is unarchived.
- Adding archive-specific UI behavior in `app/` beyond calling the shared daemon and SDK contract.
- Extending third-party `WorktreePlugin` with archive or restore hooks in v1.
- Providing indefinite post-restore snapshot retention. The archive snapshot exists only while the session is archived.

## Assumptions and Constraints

- The repository is pre-alpha. Small schema additions and archive-state refinements are acceptable.
- Git CLI commands remain the integration surface. The daemon must not read `.git` internals directly.
- Worktree snapshot and restore operate only on daemon-managed linked worktrees already recorded in `db.worktrees`.
- Sessions without a persisted worktree may still transition between archived and unarchived status, but they skip all Git-owned archive behavior.
- Archive is a user-driven lifecycle transition. The daemon should reject archive for a currently live `active` session instead of trying to preserve reconnectability.
- Restore creates a detached linked worktree intentionally. Detached restore is the mechanism that avoids new user-visible branches.
- The daemon uses the existing per-repository worktree lock so archive, restore, sync, and cleanup do not race each other.

## Terminology

- `Archived Session`
  - A daemon session whose user-facing status is `archived`.
  - Why: so the user can hide or defer the session while preserving its stored history and, when present, its archived worktree state.
- `Archivable Session Status`
  - One non-live session status that may transition into `archived`: `idle`, `blocked`, `done`, `error`, or `cancelled`.
  - Why: so unarchive can restore a truthful status without pretending the live agent runtime still exists.
- `Worktree Archive Snapshot`
  - The stash-style commit that captures tracked working tree changes and index state relative to the archived worktree `HEAD`.
  - Why: so the daemon can remove the worktree directory and later restore the same tracked dirty state.
- `Archive Ref`
  - The daemon-private Git ref that points at one `Worktree Archive Snapshot`.
  - Why: so the snapshot stays reachable without depending on `refs/stash`.
- `Archive Metadata`
  - The daemon-owned metadata file stored under the repository common Git dir while a session worktree is archived.
  - Why: so clean worktrees and snapshot refs can both be restored deterministically.
- `Restored Worktree`
  - A linked worktree recreated from archive metadata and, when present, one archive snapshot.
  - Why: so unarchive returns the user to an isolated working copy without recreating a branch.

## Proposed Design

### 1. Ownership

#### `core/daemon/src/session/manager.ts`

Owns the session lifecycle transition:

- validates that archive or unarchive is allowed for the current session status,
- unmounts worktree sync first when the worktree is still mounted,
- updates persisted session status on success,
- emits archive lifecycle diagnostics,
- exposes archive and unarchive daemon IPC methods.

#### `core/daemon/src/worktrees/archive.ts`

Owns the Git-native archive mechanics:

- capture of tracked dirty state into a stash-style commit,
- persistence under one daemon-private ref,
- archive metadata file reads and writes,
- worktree cleanup and removal,
- linked worktree recreation and snapshot apply,
- archive-state inspection.

#### `core/schema` and `core/sdk`

Own the shared contract:

- archive and unarchive request and response types,
- persisted session status support for reversible archive,
- worktree archive inspection fields surfaced to clients,
- thin SDK methods that mirror the daemon IPC surface.

### 2. Session Lifecycle Contract

Archive becomes an explicit daemon mutation instead of a pure storage flag.

`DaemonSession` gains:

```ts
type ArchivableSessionStatus = "idle" | "blocked" | "done" | "error" | "cancelled"

type DaemonSession = {
  status: DaemonSessionStatus
  archivedFromStatus: ArchivableSessionStatus | null
  // existing fields
}
```

Invariants:

- `status === "archived"` requires `archivedFromStatus !== null`
- `status !== "archived"` requires `archivedFromStatus === null`

Valid archive sources are:

- `idle`
- `blocked`
- `done`
- `error`
- `cancelled`

Archive rejects:

- `active`
- any session whose connection is still live and reconnectable

On successful archive:

- `session.status` becomes `archived`
- `session.archivedFromStatus` records the prior `Archivable Session Status`
- session history remains readable
- connection mode remains history-only or none
- live reconnectability is not preserved

On successful unarchive:

- `session.status` is restored from `session.archivedFromStatus`
- `session.archivedFromStatus` becomes `null`
- the recreated worktree remains detached
- session connection mode stays non-live

Archive on an already archived session is idempotent and returns the current archived state. Unarchive on a non-archived session fails.

If the session has no persisted worktree record:

- archive still updates `status` and `archivedFromStatus`
- the response returns `worktree: null`
- unarchive restores the prior non-live status without any Git worktree action

### 3. Worktree Model Changes

The existing persisted worktree shape is not enough to describe a restored detached worktree honestly. Add:

```ts
type SessionWorktreeHeadMode = "branch" | "detached"

type SessionWorktreeArchiveState = {
  status: "archived"
  baseOid: string
  snapshotRef: string | null
  snapshotOid: string | null
  trackedOnly: true
  includesIndex: true
  archivedAt: number
}
```

`DaemonWorktree` and `SessionWorktree` gain:

```ts
headMode: SessionWorktreeHeadMode
archive: SessionWorktreeArchiveState | null
```

Field semantics:

- `headMode`
  - `branch` for ordinary daemon-created worktrees that currently have a symbolic `HEAD`
  - `detached` for archive-restored worktrees and any reused worktree whose `HEAD` is detached
- `branchName`
  - remains the last known branch label for UX and diagnostics
  - does not imply the worktree is currently attached to that branch when `headMode === "detached"`
- `archive`
  - `null` when no archived worktree state exists
  - non-null only while the session is archived and its worktree has been snapshotted for later restore

### 4. IPC and SDK Surface

Add two daemon IPC actions and matching SDK helpers:

```ts
type ArchiveSessionRequest = DaemonSessionIdParams
type UnarchiveSessionRequest = DaemonSessionIdParams

type MutateSessionArchiveResponse = SessionIdentity & {
  session: DaemonSession
  worktree: SessionWorktree | null
  warnings: string[]
}
```

Proposed method names:

- `sessionArchive`
- `sessionUnarchive`

SDK methods:

- `sdk.session.archive(...)`
- `sdk.session.unarchive(...)`

This preserves the repo rule that shared user-facing mutation surfaces live in `core/sdk` together with the daemon contract.

### 5. Git-Owned Persistence Model

Static worktree identity stays in `db.worktrees`. Archive payload and archive lifecycle state live under the repository common Git dir.

#### Ref namespace

```text
refs/goddard/worktree-archive/<session>/snapshot
```

Rules:

- the ref points to one stash-style commit object
- the ref is absent when the archived worktree was clean
- the daemon never stores or resolves archive durability through `stash@{n}`
- `git update-ref` is always used for writes and deletes

#### Metadata path

```text
<common-git-dir>/goddard/worktree-archive/<session>.json
```

Metadata shape:

```json
{
  "sessionId": "ses_123",
  "repoRoot": "/repo",
  "worktreeDir": "/repo/.worktrees/goddard-ses_123",
  "baseOid": "abc123",
  "snapshotRef": "refs/goddard/worktree-archive/ses_123/snapshot",
  "archivedAt": 1770000000000,
  "trackedOnly": true,
  "includesIndex": true
}
```

Notes:

- `snapshotRef` is nullable in memory even if the JSON example shows a string. Clean archived worktrees persist metadata with `snapshotRef: null`.
- `baseOid` is stored even though the stash commit is parented to the original `HEAD`. Keeping it explicit improves restore clarity and error reporting.
- The archive metadata file exists only while the session is archived.

### 6. Snapshot Semantics

Archive snapshot captures:

- tracked working tree modifications
- tracked staged state in the index

Archive snapshot does not capture:

- untracked files
- ignored files

That trade is intentional. The archive feature exists to reclaim disk safely for tracked work. Preserving untracked and ignored content would either require different capture machinery or silently pin large generated directories, which works against the cleanup goal.

Snapshot capture uses:

```bash
stash_oid=$(git stash create "goddard archive $SESSION_ID")
```

If `stash_oid` is non-empty, the daemon persists:

```bash
git update-ref --create-reflog "refs/goddard/worktree-archive/$SESSION_ID/snapshot" "$stash_oid"
```

If `stash_oid` is empty, the worktree is considered clean for tracked state. Archive still proceeds, but metadata records `snapshotRef: null`.

### 7. Archive Flow

Archive order of operations:

1. Load the session and its persisted worktree record.
2. Reject if the session is live or `active`.
3. If worktree sync is mounted, unmount it first so the primary checkout is restored before archive cleanup begins.
4. Resolve the worktree repository common dir and current `HEAD` OID.
5. Capture the stash-style snapshot with `git stash create`.
6. If a snapshot OID exists, persist it under `refs/goddard/worktree-archive/<session>/snapshot` and verify the ref resolves.
7. Write archive metadata under `<common-git-dir>/goddard/worktree-archive/<session>.json`.
8. Clean the worktree with:

   ```bash
   git reset --hard HEAD
   git clean -fd
   ```

9. Remove the linked worktree with:

   ```bash
   git worktree remove <worktreeDir>
   ```

10. Update the session to `status: "archived"` and store `archivedFromStatus`.

Design decisions in this flow:

- `git clean -fd` is always part of archive cleanup. Because untracked files are not preserved in v1, archive explicitly discards them before worktree removal.
- The daemon removes the worktree using Git directly after cleanup rather than depending on plugin-specific cleanup hooks. Restore uses the same Git-native assumption.
- If any step before snapshot durability fails, the daemon leaves the worktree untouched and the session status unchanged.
- If cleanup or worktree removal fails after the snapshot ref was written, the daemon keeps the session unarchived, emits a diagnostic, and attempts to reapply the snapshot immediately before deleting archive metadata and the private ref. If that rollback fails, the daemon preserves the private ref and reports manual recovery instructions in diagnostics.

### 8. Restore Flow

Restore order of operations:

1. Load the archived session, its persisted worktree record, and the archive metadata file.
2. Verify that `session.archivedFromStatus` is present.
3. Verify that the target `worktreeDir` does not already exist as a non-empty directory or an assigned linked worktree.
4. Recreate the linked worktree at the recorded path in detached mode:

   ```bash
   git worktree add --detach <worktreeDir> <baseOid>
   ```

5. If `snapshotRef` is non-null, apply the archived tracked state:

   ```bash
   git -C <worktreeDir> stash apply --index <snapshotRef>
   ```

6. Update the persisted worktree record:
   - keep the recorded `branchName` as the pre-archive branch hint
   - set `headMode` to `detached`
7. Delete the archive metadata file and the archive ref.
8. Restore `session.status` from `session.archivedFromStatus` and clear `archivedFromStatus`.

Restore intentionally does not:

- create or attach a branch
- rerun plugin setup hooks
- reconnect the old ACP session

That keeps restore semantics narrow and predictable: recreate the isolated Git checkout, then reapply tracked dirty state.

### 9. Interaction with Existing Sync Sessions

Mounted sync already mutates both the session worktree and the primary checkout. Archive must not race or partially bypass that logic.

Rules:

- archive first checks for mounted sync state on the session worktree
- if sync is mounted, archive calls the normal sync unmount path before snapshotting
- archive fails if sync unmount fails
- restore does not automatically remount sync

This keeps archive semantics one-directional:

- sync state is ephemeral and ends before archive
- archive state is durable and survives until unarchive

### 10. Architecture and End-to-End Flow

#### Archive

```text
user archive request
  -> session manager validates status and sync state
  -> worktree archive host captures tracked snapshot
  -> archive host persists private ref + metadata
  -> archive host cleans and removes linked worktree
  -> session manager marks session archived
  -> daemon returns updated session + worktree archive state
```

#### Unarchive

```text
user unarchive request
  -> session manager validates archived session state
  -> worktree archive host recreates detached linked worktree at baseOid
  -> archive host reapplies archived snapshot when present
  -> archive host deletes archive metadata + private ref
  -> session manager restores the prior non-live session status
  -> daemon returns updated session + restored worktree state
```

## Alternatives and Tradeoffs

### Use a daemon-private ref instead of `refs/stash`

Chosen because:

- `stash@{n}` is reflog addressing, not a durable identifier
- user stash activity must not rewrite daemon restore handles
- the daemon can delete archive refs precisely when unarchive succeeds

Rejected alternative:

- storing only `stash@{n}`
  - rejected because the ordinal changes as the user's stash stack changes

### Restore detached instead of restoring a branch-backed worktree

Chosen because:

- detached restore avoids branch-list spam
- archive restore should not synthesize long-lived `refs/heads/*` entries
- the archive feature is about recovering tracked dirty state, not recreating branch workflow

Rejected alternative:

- create or recreate a session branch on restore
  - rejected because it introduces user-visible refs for an internal persistence feature

### Track only tracked changes in v1

Chosen because:

- `git stash create` is a clean fit for tracked dirty state and index state
- archive cleanup exists to reclaim disk, especially large untracked directories
- tracked-only semantics are clearer and easier to test

Rejected alternative:

- preserve untracked files too
  - rejected for v1 because it complicates capture semantics and works against the cleanup goal

### Delete archive refs after successful restore

Chosen because:

- the archive snapshot is an implementation detail of the archived state
- once the worktree exists again, retaining the ref only adds stale Git-owned state
- repeated archive later simply captures a fresh snapshot

Rejected alternative:

- keep archive refs after unarchive for later garbage collection
  - rejected because it prolongs hidden retained state without a clear user-facing need

## Failure Modes and Edge Cases

### Clean worktree archive

`git stash create` may return no OID. Archive still succeeds:

- metadata is written with `snapshotRef: null`
- the worktree is removed from disk
- restore recreates a clean detached worktree at `baseOid`

### Archive ref write failure

If `git update-ref` fails:

- archive aborts
- metadata is not written
- session status does not change
- the worktree stays on disk

### Archive while sync is mounted

If sync unmount fails:

- archive aborts
- no snapshot is written
- the session remains unarchived

### Restore path already exists

Restore fails before `git worktree add` when:

- the target path exists and is non-empty
- the path is already assigned to another linked worktree

The session stays archived and the archive metadata remains intact.

### Snapshot apply conflicts during restore

`git stash apply --index` may conflict even when restoring at the original `baseOid`, for example if the target path was partially recreated or the repository state moved unexpectedly.

When apply fails:

- the session stays archived
- the recreated worktree is left on disk for inspection
- the archive ref and metadata remain intact
- diagnostics report the restore path and Git failure output

### Missing archive metadata or legacy archived sessions

Sessions archived before this feature ships do not have archive metadata or `archivedFromStatus`.

In v1:

- unarchive fails with a clear `not restorable` error
- the daemon does not guess a replacement status or synthesize a clean worktree

### Manual worktree deletion outside the daemon

If the archived worktree directory was already deleted manually, archive is unaffected because restore relies on the recorded path and archive metadata, not the original directory contents.

If a partially restored worktree directory is deleted manually before restore completes, the next restore attempt behaves like a fresh restore if the target path is absent.

## Testing and Observability

Required daemon tests:

- archive a dirty tracked worktree and verify the worktree directory is removed, the private ref exists during archive, and the session status becomes `archived`
- unarchive a dirty tracked worktree and verify the restored worktree is detached and dirty state is reapplied
- archive and unarchive a clean worktree with `snapshotRef: null`
- verify untracked files are discarded during archive
- verify archive first unmounts sync and restores the primary checkout
- verify restore failure leaves archive metadata intact and the session still archived
- verify legacy archived sessions fail unarchive with a clear error
- verify SDK archive and unarchive helpers send the expected daemon IPC methods

Recommended diagnostics:

- `worktree.archive_requested`
- `worktree.archive_snapshot_captured`
- `worktree.archived`
- `worktree.archive_failed`
- `worktree.unarchive_requested`
- `worktree.restored`
- `worktree.restore_failed`
- `worktree.archive_warning`

Each event should include at least:

- `sessionId`
- `repoRoot`
- `worktreeDir`
- `baseOid`
- `snapshotRef` when present

## Rollout and Migration

No config flag is required in v1. The behavior is attached to the new daemon archive and unarchive mutations.

Migration notes:

- existing `DaemonSession` records gain `archivedFromStatus: null`
- existing `DaemonWorktree` records gain `headMode: "branch"` on first rewrite unless a reused worktree is already detached
- sessions already in `status: "archived"` before rollout remain archived but are not automatically restorable

Rollback:

- disabling the archive and unarchive mutations stops new archive snapshots from being created
- existing archive refs and metadata files can be cleaned with a one-off maintenance script if rollback is required

## Open Questions

None for the core v1 flow.

## Ambiguities and Blockers

- AB-1 - Non-blocking - Third-party plugin side effects are not replayed on restore
  - Affected area: Proposed Design / Worktree Plugin boundary
  - Issue: Restore recreates a plain linked worktree at the recorded path and does not call plugin-specific setup logic.
  - Why it matters: a future plugin could rely on durable non-Git side effects that this feature would not recreate.
  - Next step: keep v1 semantics Git-only and extend `WorktreePlugin` with explicit restore hooks only if a concrete plugin needs them.

- AB-2 - Non-blocking - Crash recovery in the middle of archive cleanup is best-effort
  - Affected area: Failure Modes and Operational Recovery
  - Issue: v1 attempts immediate rollback when cleanup fails after snapshot capture, but it does not add a separate startup reconciler for archive operations interrupted by daemon crash.
  - Why it matters: a crash at the wrong point could leave a durable archive ref without a completed archived session transition.
  - Next step: add archive-operation reconciliation only if real-world testing shows this is more than a rare local failure mode.
