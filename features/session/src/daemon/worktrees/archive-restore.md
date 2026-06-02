# Archived Session Worktree Snapshot and Restore

Status: proposed

## Overview

Daemon-managed sessions can own isolated linked Git worktrees. Archiving a session should let the user reclaim that worktree's disk footprint without losing the user work that is expected to survive the archive.

This design adds one daemon-owned archive flow for session worktrees:

1. when the user archives a session, the daemon snapshots the worktree's tracked changes and untracked non-ignored files into a private Git ref,
2. the daemon cleans and removes the linked worktree from disk, and
3. when the user unarchives that session later, the daemon recreates a linked worktree and reapplies the snapshot.

The archive payload is Git-native. It uses a stash-style commit captured relative to the worktree `HEAD`, stored under a daemon-private ref namespace instead of the user's stash stack. Restore recreates the worktree in detached mode so the daemon does not create user-visible branches.

This design is scoped to daemon-managed linked worktrees and session archive lifecycle. It does not reconnect live ACP transports or resume agent execution.

## Current Architecture

The rebased repository moved session behavior into the session feature package. Archive/restore ownership should follow that boundary:

- `features/session/src/daemon/manager.ts` owns session lifecycle mutations, persisted session/worktree records, diagnostics, and connection-state semantics.
- `features/session/src/daemon/worktree.ts` owns reusable session worktree helpers and completion-state inspection.
- `features/session/src/daemon/worktrees/index.ts` owns worktree creation and deletion through worktree plugins.
- `features/session/src/daemon/worktrees/archive.ts` should own the Git-native archive host introduced by this feature.
- `features/session/src/daemon-ipc.ts` owns session feature IPC routes.
- `features/session/src/schema.ts` owns session feature request, response, and worktree payload schemas.
- `features/session/src/sdk.ts` owns session feature SDK wrappers, which are composed into `core/sdk/src/sdk.ts`.
- `core/schema/src/daemon/store.ts` owns persisted daemon session and worktree record schemas.

The previous pre-rebase plan referenced `core/daemon/src/session/manager.ts`, `core/daemon/src/worktrees/*`, and a daemon-managed worktree sync host. Those assumptions are obsolete. The current tree has no old worktree-sync host to coordinate with, so archive/restore should own its own sequencing and safety checks.

## Goals

- Allow a user archive action to reclaim the on-disk session worktree without losing tracked modifications, staged state, or untracked non-ignored files.
- Restore the archived worktree on user unarchive without creating a visible `refs/heads/*` branch.
- Keep the durable snapshot outside `refs/stash` so it is unaffected by user stash churn.
- Preserve enough metadata to restore clean archived worktrees that have no snapshot commit.
- Expose archive and unarchive through the session feature IPC and SDK surfaces.
- Expose user-facing archive and unarchive controls in both session detail/chat and the session list.

## Non-Goals

- Preserving ignored files in v1.
- Reconnecting or reviving live agent execution when a session is unarchived.
- Extending third-party `WorktreePlugin` with archive or restore hooks in v1.
- Creating normal branches for archived or restored worktrees.
- Adding bulk archive behavior unless it falls out of an existing session-list pattern without extra product decisions.

## Assumptions and Constraints

- Git CLI commands remain the integration surface. The daemon must not read `.git` internals directly.
- Worktree snapshot and restore operate only on daemon-managed linked worktrees already recorded in `db.worktrees`.
- Sessions without a persisted worktree may still transition between archived and unarchived status, but they skip Git-owned archive behavior.
- Archive is a user-driven lifecycle transition. The daemon should reject archive for currently live sessions when the session is still active or reconnectable.
- Unarchive must not make a stale live session appear live again. Safe terminal/non-live prior statuses may be restored; stale live or reconnectable prior states restore to a safe non-live status.
- Restore creates a detached linked worktree intentionally. Detached restore is the mechanism that avoids new user-visible branches.
- The snapshot durability boundary is the private Git ref. The daemon must not remove the worktree until the ref or clean-archive metadata is durably recorded.

## Terminology

- `Archived Session`
  - A daemon session whose user-facing status is `archived`.
  - Why: so the user can hide or defer the session while preserving stored history and, when present, archived worktree state.
- `Worktree Archive Snapshot`
  - The stash-style commit that captures tracked working tree changes, index state, and untracked non-ignored files relative to the archived worktree `HEAD`.
  - Why: so the daemon can remove the worktree directory and later restore the same user-visible dirty state.
- `Archive Ref`
  - The daemon-private Git ref that points at one `Worktree Archive Snapshot`.
  - Why: so the snapshot stays reachable without depending on `refs/stash`.
- `Archive Metadata`
  - The daemon-owned persisted metadata attached to the worktree record while a session worktree is archived.
  - Why: so clean worktrees, snapshot refs, original `HEAD`, restore paths, and user-facing state can be restored deterministically.
- `Restored Worktree`
  - A linked worktree recreated from archive metadata and, when present, one archive snapshot.
  - Why: so unarchive returns the user to an isolated working copy without recreating a branch.

## Proposed Design

### 1. Session Lifecycle Contract

Archive is an explicit daemon mutation instead of a pure storage flag.

`DaemonSession` gains prior-status metadata:

```ts
type SafeArchivedFromStatus = "idle" | "blocked" | "done" | "error" | "cancelled"

type DaemonSession = {
  status: DaemonSessionStatus
  archivedFromStatus: SafeArchivedFromStatus | null
  // existing fields
}
```

Invariants:

- `status === "archived"` requires `archivedFromStatus !== null`
- `status !== "archived"` requires `archivedFromStatus === null`
- archive on an already archived session is idempotent
- unarchive on a non-archived session fails

Archive source handling:

- safe non-live statuses record themselves in `archivedFromStatus`
- active or reconnectable sessions are rejected
- stale live statuses discovered from persisted records after reconciliation are normalized to a safe non-live restore status

On successful archive:

- `session.status` becomes `archived`
- `session.archivedFromStatus` records the safe restore status
- session history remains readable
- connection mode remains history-only or none
- live reconnectability is not preserved

On successful unarchive:

- `session.status` is restored from `session.archivedFromStatus`
- `session.archivedFromStatus` becomes `null`
- a worktree, when present, is recreated as detached
- session connection mode stays non-live

If the session has no persisted worktree record:

- archive still updates `status` and `archivedFromStatus`
- the response returns `worktree: null`
- unarchive restores the safe prior status without any Git worktree action

### 2. Worktree Model Changes

The existing persisted worktree shape is not enough to describe archived and detached-restored state. Add:

```ts
type SessionWorktreeHeadMode = "branch" | "detached"

type SessionWorktreeArchiveState = {
  status: "archived"
  baseOid: string
  snapshotRef: string | null
  snapshotOid: string | null
  includesIndex: true
  includesUntracked: true
  includesIgnored: false
  archivedAt: string
  originalWorktreeDir: string
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
  - non-null only while the session is archived and its worktree has been snapshotted or recorded clean for later restore

### 3. IPC and SDK Surface

Add two session feature IPC actions and matching SDK helpers:

```ts
type ArchiveSessionRequest = DaemonSessionIdParams
type UnarchiveSessionRequest = DaemonSessionIdParams

type MutateSessionArchiveResponse = SessionIdentity & {
  session: DaemonSession
  worktree: SessionWorktree | null
  warnings: string[]
}
```

Proposed route shape:

- `client.session.archive({ id })`
- `client.session.unarchive({ id })`

SDK methods:

- `sdk.session.archive({ id })`
- `sdk.session.unarchive({ id })`

The routes belong in `features/session/src/daemon-ipc.ts`, using the same resource-style route tree as `session.create`, `session.changes`, and `session.worktree.get`. The SDK wrappers belong in `features/session/src/sdk.ts` and are composed into `core/sdk/src/sdk.ts` through the existing session feature plugin.

### 4. Git-Owned Snapshot Model

Static worktree identity stays in `db.worktrees`. The archive payload is owned by Git through a daemon-private ref.

#### Ref namespace

```text
refs/goddard/worktree-archive/<session-id>/snapshot
```

Rules:

- the ref points to one stash-style commit object
- the ref is absent when the archived worktree was clean
- the daemon never stores or resolves archive durability through `stash@{n}`
- `git update-ref --create-reflog` is used to create or update refs
- `git update-ref -d` is used when a snapshot is garbage-collected

#### Snapshot construction

The host should use Git's stash commit shape because `git stash apply` already knows how to restore working-tree state, index state, and untracked files when the commit is shaped like a stash entry.

Important constraint: `git stash create` is useful for tracked working-tree and index state, but it does not provide a portable `--include-untracked` mode. The implementation must not assume `git stash create --include-untracked` works.

Archive should construct a stash-shaped commit without writing to `refs/stash`:

1. capture the tracked/index stash payload with `git stash create`
2. capture untracked non-ignored files into an untracked tree using Git plumbing
3. create or rewrite the final stash-shaped commit so its parents are:
   - parent 1: archived worktree `HEAD`
   - parent 2: index-state commit
   - parent 3: untracked-files commit, when untracked non-ignored files exist
4. store the final snapshot commit under the daemon-private ref

```bash
git update-ref --create-reflog "refs/goddard/worktree-archive/<session-id>/snapshot" "$stash_oid"
```

The host may use another implementation if it preserves the same externally observable semantics without touching the user's stash stack. The implementation must prove with tests that untracked non-ignored files are restored, ignored files are not restored, and `refs/stash` is unchanged.

The host must treat an empty stash OID as a valid clean archive outcome:

- record clean archive metadata
- do not create a snapshot ref
- proceed with worktree removal only after metadata is persisted

### 5. Cleanup and Restore

Archive cleanup sequence:

1. resolve and record the worktree `HEAD` OID
2. create the snapshot if the worktree has restorable state
3. persist the snapshot under the private ref
4. persist archive metadata in the worktree record
5. clean the worktree with Git commands
6. remove the linked worktree through the existing worktree plugin cleanup path
7. update the session status to `archived`

Restore sequence:

1. verify the session is archived
2. verify archive metadata and private ref, when present
3. choose the restore path from the persisted worktree record or product policy
4. create a linked worktree at the archived base OID with detached `HEAD`
5. apply the snapshot ref with index restoration
6. keep the snapshot ref until explicit cleanup policy removes it
7. clear archive metadata and restore a safe non-live session status

Restore failure handling:

- keep the snapshot ref
- keep the partially restored worktree for inspection when creation succeeded
- keep the session archived or mark a partial/conflicted restore state in diagnostics
- report whether failure happened during worktree creation or snapshot application

### 6. Archived Changes

`features/session/src/daemon/changes.ts` currently reads changes from the live worktree path when a persisted worktree exists. Archived sessions may no longer have that directory.

Changes behavior should be:

- non-archived worktree sessions continue to read from the worktree directory
- archived clean worktrees return empty changes
- archived dirty worktrees produce changes from the archive snapshot relative to the archived base OID
- missing archived worktree directories are non-fatal

This keeps session history and change inspection useful after disk cleanup.

### 7. UI Surface

The app should expose archive and unarchive controls in:

- session detail/chat
- session list

UI expectations:

- archived state is visible in both surfaces
- archive is disabled or clearly rejected for unsafe live/reconnectable sessions
- unarchive restores the worktree before showing it as available
- no bulk archive behavior is required unless it naturally fits an existing session-list pattern without additional product decisions

## Failure Modes

### No snapshot payload

If there are no tracked changes and no untracked non-ignored files, archive still succeeds for worktree cleanup by recording clean archive metadata. No private snapshot ref is created.

### Private ref write failed

If the private ref write fails:

- do not clean or remove the worktree
- leave the session unarchived
- report archive failure

The private ref or clean metadata is the durability boundary.

### Worktree removal failed

If worktree removal fails after the snapshot is durable:

- keep the session unarchived unless cleanup has clearly completed
- keep the private ref for recovery
- report cleanup failure with the worktree path

### Restore path already exists

Fail before creating the worktree unless the path is empty and the product explicitly allows reuse.

### Worktree creation failed

Possible reasons include stale worktree admin state, an occupied path, or plugin failure. The daemon should report diagnostics and keep archive metadata intact.

### Snapshot apply conflicts

Keep the snapshot ref, keep the partially restored worktree, and surface the path plus conflict state so the user can inspect or resolve it.

### Linked worktree directory was manually deleted

Missing archived worktree directories are non-fatal. Missing non-archived worktree directories should continue to be treated as existing lifecycle or cleanup failures.

## Retention and Garbage Collection

Private refs keep snapshots reachable until the daemon deletes them. That is intentional while a session is archived.

Initial policy:

- keep the snapshot ref while the session remains archived
- keep the snapshot ref after conflicted restore
- delete the snapshot ref only after a successful restore and explicit cleanup policy says the archived payload is no longer needed

## Test Plan

- Contract tests for archive/unarchive schema and SDK methods.
- Host tests proving tracked changes, staged state, and untracked non-ignored files are restored.
- Host tests proving ignored files are not restored.
- Host tests proving no normal branches or `refs/stash` entries are created.
- Manager tests for status-only archive/unarchive.
- Manager tests for worktree archive cleanup after durability is established.
- Restore tests for detached worktrees and conflict preservation.
- Changes tests for archived dirty snapshots, archived clean worktrees, missing archived directories, and unchanged non-archived behavior.
- App tests or type coverage for session detail/chat and session list controls.
