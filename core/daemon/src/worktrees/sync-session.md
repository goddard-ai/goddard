# Daemon Worktree Sync Sessions

## Overview

The daemon can already provision one linked session worktree, but it cannot yet keep that worktree and the primary checkout synchronized in a reversible way.

This design adds one Git-native sync session per daemon session:

- all daemon-managed worktrees are linked Git worktrees
- sync operates only between the primary checkout and one linked session worktree
- dirty state is captured as stash-shaped snapshot commits
- every sync cycle rebuilds both checkouts from `baseOid + resultSnapshot`
- non-conflicting edits from both sides are preserved
- when the same path or hunk conflicts, the session worktree wins

This phase intentionally excludes `app/` changes.

## Context

### Ownership boundaries

- `core/daemon/src/worktrees/`
  - Owns daemon-managed worktree creation and cleanup.
  - `createWorktree()` resolves plugins, provisions one worktree, and returns durable metadata.
- `core/daemon/src/session/worktree.ts`
  - Owns reuse and cleanup of persisted session worktrees.
- `core/daemon/src/session/manager.ts`
  - Owns `resolveLaunchWorktree()` and persists session worktree metadata in `db.worktrees`.
- `core/worktree-plugin`
  - Owns the shared plugin contract used by built-in and third-party worktree plugins.
- `core/schema` and `core/sdk`
  - Expose session worktree inspection and sync control surfaces.

### Design basis

Daemon-owned worktree behavior already lives under `core/daemon/src/worktrees/`, so sync belongs there instead of in plugins or in the app layer.

The key simplifying decisions are:

- daemon-managed worktrees are real linked worktrees
- custom plugins must also return linked worktrees attached to the source repository
- one sync strategy is better than topology-specific behavior
- the session worktree is the preferred side when automatic merge resolution is required

## Goals

- Keep ordinary `worktree.enabled` sessions working as isolated daemon-managed worktrees.
- Support one reversible mount between the primary checkout and one daemon `Session Worktree`.
- Allow sync to be enabled at session creation time or later for an existing linked session worktree.
- Allow a later sync mount on the same primary checkout to replace the earlier mounted sync session without user intervention.
- Preserve the primary checkout's pre-mount state and restore it on unmount.
- Automatically sync dirty state in both directions while the daemon owns the live session.
- Preserve non-conflicting edits from both sides during one sync cycle.
- Prefer the session worktree when one sync cycle encounters overlapping edits.
- Rebuild both checkouts from `baseOid + resultSnapshot` on every sync instead of incrementally applying over prior mirrored state.
- Persist Git-owned sync state outside daemon memory so the daemon can recover and inspect mounted sessions after restart.
- Expose the capability through `core/schema`, `core/daemon`, and `core/sdk` without requiring `app/` work.

## Non-Goals

- Interactive conflict resolution or a merge UI.
- Syncing ignored files.
- Supporting more than one mounted sync session against the same primary checkout at the same time.
- Preserving the primary checkout's branch attachment while mounted.
- Deleting the session worktree itself during unmount.
- Extending the third-party `WorktreePlugin` contract beyond setup and cleanup.

## Assumptions and Constraints

- The daemon session id is the sync session id. No second identifier is introduced.
- The primary checkout is the existing `repoRoot` resolved from `resolveGitRepoRoot(params.request.cwd)`.
- `createWorktree()` already enforces the linked-worktree invariant for built-in and custom plugins.
- One primary checkout can participate in at most one mounted sync session at a time because the primary checkout is detached and mirrored while mounted.
- When a second sync mount targets the same primary checkout, the daemon silently unmounts the older mounted session and then mounts the newer one.
- Git CLI commands remain the integration mechanism. The implementation must use supported plumbing commands such as `git rev-parse`, `git update-ref`, `git stash`, `git reset`, `git clean`, and `git merge` instead of reading `.git` internals directly.
- Automatic sync is best-effort and watcher-driven while the daemon owns one live session. Manual sync remains available as a fallback and test hook.
- Root config remains unchanged in phase 1 except for any optional plugin behavior already supported today.
- The repository is pre-alpha. Optional schema additions and small API shape changes are acceptable.

## Terminology

- `Primary Checkout`
  - The checkout from which the daemon session was launched.
  - In current daemon metadata this is `repoRoot`.
- `Session Worktree`
  - The daemon-provisioned linked worktree stored in `worktreeDir`.
  - The daemon session runs from `effectiveCwd` inside this checkout.
- `Linked Worktree`
  - A checkout that shares one Git common dir with another checkout.
- `Sync Session`
  - The Git-native linkage between one `Primary Checkout` and one `Session Worktree`, keyed by daemon session id.
- `Base OID`
  - The commit OID that both checkouts must continue to reference while the sync session is mounted.
- `Snapshot Commit`
  - A stash-shaped commit that captures dirty tracked state, index state, and untracked files relative to one checkout's `HEAD`.
- `Materialized Commit`
  - A temporary ordinary commit created in a scratch worktree from one snapshot commit so Git's normal merge machinery can be used.
- `Result Snapshot`
  - The stash-shaped snapshot that represents the merged dirty state that should be applied back to both checkouts.
- `Conflict Preference`
  - The rule used when one sync cycle cannot keep both sides' edits for the same content. In phase 1, the session worktree wins.
- `Recovery Snapshot`
  - The retained primary-side snapshot from a sync cycle where both sides contributed inputs. It exists so displaced primary edits can still be inspected or recovered manually.
- `Sync Cycle`
  - One locked operation that captures both sides, computes one result snapshot, and rebuilds both checkouts from that result.

## Proposed Design

### 1. Ownership

#### `core/daemon/src/worktrees/`

Owns the Git-native mechanics:

- shared ref namespace creation and cleanup
- metadata file persistence under the Git common dir
- mount, inspect, sync, and unmount operations
- scratch-worktree merge execution
- recovery snapshot retention and Git-level cleanup behavior

This logic lives outside `WorktreePlugin`. Sync semantics depend on Git common-dir invariants that the daemon now requires for all managed worktrees.

#### `core/worktree-plugin`

Remains intentionally narrow:

- defines `WorktreePlugin`
- supports setup and cleanup for built-in and third-party worktree strategies
- does not own sync session state, Git ref management, or merge semantics

#### `core/daemon/src/session/manager.ts`

Owns orchestration:

- requesting sync mount during session creation or later explicit activation
- mapping daemon session ids to sync sessions
- starting and stopping one debounced auto-sync runtime for each mounted live session
- surfacing sync state through `getWorktree()`
- exposing mount, manual sync, and unmount IPC actions
- recording session diagnostics for sync lifecycle events
- auto-unmounting mounted sync sessions during explicit shutdown and reconciliation

#### `core/schema` and `core/sdk`

Own the typed control plane:

- sync-enabled session creation params
- sync mount, sync, and unmount request and response shapes
- thin SDK methods that mirror the daemon IPC contract

### 2. Worktree host invariant

`createWorktree()` already returns only linked worktrees.

That invariant applies to:

- the built-in `default` plugin
- the built-in `worktrunk` plugin
- any custom plugin loaded through `worktrees.plugins`

The daemon rejects plugin output unless:

- the returned path is a Git worktree
- the returned path shares the same Git common dir as the source repository
- the returned path is not the repository root itself

This keeps sync strategy singular and removes the need for topology-specific behavior.

### 3. New daemon-owned sync host

Add one module under `core/daemon/src/worktrees/`, tentatively `sync.ts`, that owns sync session operations.

```ts
type WorktreeSyncConflictPreference = "worktree"
type WorktreeSyncStatus = "mounted"

interface WorktreeSyncSessionState {
  sessionId: string
  status: WorktreeSyncStatus
  conflictPreference: WorktreeSyncConflictPreference
  primaryDir: string
  worktreeDir: string
  commonDir: string
  baseOid: string
  primaryOriginalHeadOid: string
  primaryOriginalSymbolicRef: string | null
  primaryOriginalBranchTipOid: string | null
  primaryLatestSnapshotOid: string | null
  worktreeLatestSnapshotOid: string | null
  resultSnapshotOid: string | null
  primaryRecoverySnapshotOid: string | null
  lastSyncAt: number | null
}

class WorktreeSyncSessionHost {
  constructor(input: { sessionId: string; primaryDir: string; worktreeDir: string })

  inspect(): Promise<WorktreeSyncSessionState | null>
  mount(): Promise<WorktreeSyncSessionState>
  syncOnce(input?: { reason?: string }): Promise<{
    state: WorktreeSyncSessionState
    warnings: string[]
  }>
  unmount(): Promise<{ state: null; warnings: string[] }>
}
```

The host is stateless between calls. Each method re-reads shared metadata and refs, then performs one locked Git operation.

### 4. Git-owned persistence model

Each sync session uses the daemon session id as its namespace suffix.

#### Shared ref namespace

```text
refs/goddard/worktree-sync/<session>/base
refs/goddard/worktree-sync/<session>/primary/pre_mount
refs/goddard/worktree-sync/<session>/primary/latest
refs/goddard/worktree-sync/<session>/worktree/latest
refs/goddard/worktree-sync/<session>/result/latest
refs/goddard/worktree-sync/<session>/primary/recovery/latest
```

Rules:

- refs only store commit OIDs
- absent refs represent the clean snapshot state
- `git update-ref` is always used for writes
- compare-and-swap semantics are used when updating existing refs after a state transition

#### Shared metadata file

Path:

```text
<common-git-dir>/goddard/worktree-sync/<session>.json
```

Shape:

```json
{
  "sessionId": "ses_123",
  "primaryDir": "/repo",
  "worktreeDir": "/repo/.worktrees/goddard-ses_123",
  "commonDir": "/repo/.git",
  "baseOid": "abc...",
  "status": "mounted",
  "conflictPreference": "worktree",
  "primaryOriginalHeadOid": "def...",
  "primaryOriginalSymbolicRef": "refs/heads/main",
  "primaryOriginalBranchTipOid": "def...",
  "mountedAt": 1770000000000,
  "lastSyncAt": null
}
```

The metadata file is the non-OID source of truth. The daemon can reconstruct live sync state after restart by reading this file and the shared refs.

#### Kindstore persistence

Kindstore remains the durable source for static worktree identity only:

- `repoRoot`
- `requestedCwd`
- `effectiveCwd`
- `worktreeDir`
- `branchName`
- `poweredBy`

Snapshot OIDs and mount lifecycle state do not live in `db.worktrees`. They stay Git-owned so the daemon can recover from restart without introducing a second mutable source of truth.

#### Locking

Use one repo-scoped lock directory under the same common dir:

```text
<common-git-dir>/goddard/worktree-sync/lock
```

Behavior:

- lock acquisition uses atomic directory creation
- lock contents record pid, hostname, and creation time
- stale locks are reclaimable when the owning pid is no longer alive

This keeps the implementation dependency-free and portable.

## API and Interface Specification

### Session creation request

Extend `SessionWorktreeParams` in `core/schema/src/daemon/sessions.ts` and `core/schema/src/session-server.ts`:

```ts
const SessionWorktreeSyncParams = z.object({
  enabled: z.boolean().optional(),
})

const SessionWorktreeParams = z.object({
  enabled: z.boolean().optional(),
  sync: SessionWorktreeSyncParams.optional(),
})
```

Semantics:

- `worktree.enabled: true` with no `sync` block preserves today's isolated-session behavior.
- `worktree.sync.enabled: true` requires `worktree.enabled: true`.
- sync-enabled session creation mounts untracked-inclusive sync before the agent starts.

### Persisted session worktree response

Extend `SessionWorktree` with:

```ts
const SessionWorktreeSyncState = z.strictObject({
  status: z.literal("mounted"),
  conflictPreference: z.literal("worktree"),
  baseOid: z.string(),
  primaryLatestSnapshotOid: z.string().nullable(),
  worktreeLatestSnapshotOid: z.string().nullable(),
  resultSnapshotOid: z.string().nullable(),
  primaryRecoverySnapshotOid: z.string().nullable(),
  lastSyncAt: z.number().int().nullable(),
})

const SessionWorktree = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
  sync: SessionWorktreeSyncState.nullable(),
})
```

`session.worktree.get` stays the read endpoint, but `SessionManager.getWorktree()` now merges persisted `db.worktrees` metadata with live sync-session state from the Git metadata file when present.

### New daemon control routes and IPC actions

Add three daemon-owned mutations:

- `POST sessions/:id/worktree/sync/mount`
  - payload: `{ id }`
  - mounts one existing linked session worktree against the primary checkout
- `POST sessions/:id/worktree/sync`
  - forces one immediate sync cycle using the same logic as the background auto-sync runtime
- `POST sessions/:id/worktree/sync/unmount`
  - removes the sync session and restores the primary checkout

All three routes return:

```ts
type MutateSessionWorktreeResponse = SessionIdentity & {
  worktree: SessionWorktree | null
  warnings: string[]
}
```

The SDK adds thin methods:

- `sdk.session.mountWorktreeSync(...)`
- `sdk.session.syncWorktree(...)`
- `sdk.session.unmountWorktree(...)`

No `app/` surface is added in this phase.

## Behavioral Semantics

### 1. Mount and activation

Sync can be mounted either during session creation or later for one already-provisioned linked session worktree.

`mount()` performs:

1. Acquire the repo lock.
2. Look for any other mounted sync session that already targets the same primary checkout.
3. If one exists and it is not the same session id:
   - stop that session's auto-sync runtime when it is live
   - unmount it with the normal unmount path while still holding the repo lock
   - clear its live sync state so later `session.worktree.get` reads report an ordinary session worktree again
   - continue without surfacing an error to the caller that requested the newer mount
4. Verify both directories still belong to the same Git common dir.
5. Read:
   - `worktreeHead = git -C <worktreeDir> rev-parse HEAD`
   - `primaryHead = git -C <repoRoot> rev-parse HEAD`
   - `primarySymbolicRef = git -C <repoRoot> symbolic-ref -q HEAD || true`
6. Set `baseOid = worktreeHead`.
7. Capture the primary checkout's pre-mount dirty state into `primary/pre_mount`:
   - `git stash push -u`, `rev-parse stash@{0}`, `stash apply --index`, `stash drop`
8. Record metadata and initialize shared refs.
9. Move the primary checkout to a detached `HEAD` at `baseOid`.
10. Reset the primary checkout to `baseOid` with `git reset --hard <baseOid>` and `git clean -fd`.
11. Compute and apply one initial result using the same sync-cycle logic before releasing the mount lock.

Important consequence:

- the worktree's current commit becomes the mounted `baseOid`
- the primary checkout's pre-mount dirty state is preserved for later unmount, not merged into live sync state
- if sync is activated later for an existing worktree session, the worktree still wins the mount boundary by defining `baseOid`
- if another session later mounts against the same primary checkout, this mounted session is automatically replaced and becomes an ordinary non-sync session again

The session worktree may remain on its existing branch name, but while mounted its `HEAD` commit must continue to equal `baseOid`. If either checkout advances `HEAD`, later sync cycles fail until unmount.

### 2. Automatic change detection and scheduling

While the daemon owns a live mounted session, `SessionManager` runs one debounced auto-sync runtime:

- one watcher is attached to the primary checkout
- one watcher is attached to the session worktree
- `.git/` activity is ignored
- repeated filesystem events are coalesced behind one short debounce window
- if a sync cycle is already running, new events mark one rerun request instead of starting a concurrent cycle

The manual `POST sessions/:id/worktree/sync` route uses the same scheduler but skips the debounce delay.

Watcher behavior is best-effort:

- if watcher setup fails, the session remains mounted and manual sync still works
- if a watcher later degrades, the daemon records diagnostics and stops automatic triggering for that mounted session
- Git-owned mount state is still recoverable because the scheduler is not the source of truth

### 3. Snapshot capture

Capture tracked, index, and untracked state with the transient stash round trip:

```bash
git stash push -u -m "goddard:worktree-sync:<session>:<side>"
git rev-parse stash@{0}
git stash apply --index <oid>
git stash drop stash@{0}
```

Properties:

- includes untracked files
- restores the source checkout to its prior visible state after capture
- never leaves automation-created entries in the user's stash stack

Ignored files are never part of the sync contract.

### 4. Result computation

Every sync cycle captures the current nullable snapshot state of both checkouts:

- `primary/latest`
- `worktree/latest`

Then it computes one `result/latest` snapshot:

- if both sides are clean, `result/latest` is deleted
- if only one side is dirty, `result/latest` becomes that side's snapshot OID
- if both sides are dirty, the daemon computes one merged result in a temporary scratch linked worktree created at `baseOid`

The scratch merge path is:

1. Create a temporary linked worktree detached at `baseOid`.
2. Materialize the worktree snapshot into one ordinary commit whose parent is `baseOid`.
3. Reset the scratch worktree to `baseOid`.
4. Materialize the primary snapshot into one ordinary commit whose parent is `baseOid`.
5. Reset the scratch worktree again and check out the worktree materialized commit as `ours`.
6. Merge the primary materialized commit with:

```bash
git merge -X ours --no-commit <primary-materialized-commit>
```

Semantics:

- non-conflicting edits from both sides survive the merge
- overlapping textual conflicts prefer the worktree version because the scratch worktree starts from the worktree materialized commit

If Git still leaves unresolved entries after `-X ours`:

- the daemon resolves each remaining conflicted path by checking out the worktree side for that path
- the daemon stages those resolutions
- the daemon records one warning for the cycle

After the scratch worktree represents the final merged dirty state, the daemon captures that state back into `result/latest` with the same transient `git stash push -u` round trip.

The temporary scratch worktree is always removed before the cycle returns, whether the cycle succeeds or fails.

Whenever both sides contribute dirty inputs to one cycle, the daemon also stores the primary snapshot into `primary/recovery/latest`. This is deliberately conservative. The implementation does not try to distinguish harmless overlap from displaced primary edits.

### 5. Sync cycle application

`syncOnce()` performs:

1. Acquire the repo lock.
2. Verify the sync session exists.
3. Verify both `HEAD`s still equal `baseOid`.
4. Capture the current nullable snapshot state of both sides.
5. Compute `result/latest`.
6. Rebuild the primary checkout from `baseOid`:
   - `git reset --hard <baseOid>` for tracked state
   - `git clean -fd` for untracked state
   - `git stash apply --index <resultSnapshotOid>` when one exists
7. Rebuild the session worktree with the same process.
8. Update `lastSyncAt` and return live sync state plus warnings.

This model does not incrementally stack sync output on top of prior mirrored state. Each cycle reconstructs both checkouts from the same base and the same computed result snapshot, so both sides converge to one identical dirty state.

### 6. Untracked-inclusive semantics

Sync deliberately includes untracked files and directories, but still excludes ignored files.

Consequences:

- mount makes the primary checkout both tracked-clean and untracked-clean before the first mirrored apply
- one-sided sync cycles preserve untracked additions naturally through `git stash apply --index`
- two-sided sync cycles may turn formerly untracked additions into staged tracked additions inside the merged result snapshot; correctness is defined by file contents and deletions, not by preserving the original tracked-vs-untracked classification
- ignored files remain local-only and are never part of the sync contract

### 7. Unmount

`unmount()` performs:

1. Acquire the repo lock.
2. Stop the auto-sync runtime for that session.
3. Rebuild both checkouts to a clean `baseOid` state first.
4. Restore the primary checkout's original `HEAD` identity:
   - if the original head was detached, detach back to `primaryOriginalHeadOid`
   - if the original head was symbolic and the branch still points to `primaryOriginalBranchTipOid`, switch back to that branch
   - if the original branch moved, detach to `primaryOriginalHeadOid` and emit a warning
5. Reapply `primary/pre_mount` when it exists.
6. Delete the shared refs.
7. Delete the sync metadata file.

Unmount removes the logical sync link only. It does not delete the session worktree directory or the daemon worktree record.

### 8. Daemon shutdown and reconciliation

Mounted sync sessions must not leave the primary checkout detached indefinitely when the daemon intentionally ends the owning session.

Daemon behavior:

- `session.shutdown` first attempts `unmount()`
- if unmount succeeds, session shutdown proceeds normally
- if unmount fails, `session.shutdown` returns `success: false` and records one diagnostic event
- startup reconciliation attempts best-effort unmount for any persisted session whose sync metadata file still exists but whose live daemon session is gone
- if reconciliation unmount fails, the daemon records diagnostics and marks the session record with an error message describing the manual recovery requirement

This differs from ordinary session-worktree cleanup. The worktree directory still follows the existing explicit cleanup model.

## Architecture and End-to-End Flow

### Create or mount path

1. SDK or another daemon client calls `session.create` with `worktree.enabled: true` and optionally `worktree.sync.enabled: true`, or later calls `session.mountWorktreeSync({ id })`.
2. `SessionManager.resolveLaunchWorktree()` asks `createWorktree()` for one session worktree when needed.
3. `createWorktree()` persists the static worktree identity through `db.worktrees`.
4. The daemon constructs `WorktreeSyncSessionHost` with:
   - `sessionId`
   - `primaryDir = repoRoot`
   - `worktreeDir`
5. `mount()` captures pre-mount state, detaches the primary checkout, initializes shared refs, and runs one initial sync cycle.
6. The agent process starts in `effectiveCwd` inside the session worktree, or the already-running session gains one mounted sync runtime.
7. `SessionManager` starts the per-session auto-sync scheduler.
8. `SessionManager.getWorktree()` reads the persisted worktree record, then overlays live sync-session state from Git.

### Sync path

1. The auto-sync scheduler receives one debounced filesystem event, or the SDK calls `session.syncWorktree({ id })`.
2. The daemon validates that the session has a mounted sync-enabled worktree.
3. The daemon loads the Git-owned sync session host by session id.
4. `syncOnce()` captures both nullable snapshot states, computes one result snapshot, rebuilds both checkouts from `baseOid`, and returns warnings when the worktree-preferred conflict rule displaced primary content.
5. The daemon records one diagnostic event and returns the updated `SessionWorktree`.

### Unmount path

1. SDK calls `session.unmountWorktree({ id })` or the daemon triggers unmount during shutdown or reconciliation.
2. The daemon stops the auto-sync scheduler for that session.
3. The daemon loads the Git-owned sync session host and calls `unmount()`.
4. The primary checkout returns to its original tracked state and original `HEAD` identity when possible.
5. The sync metadata file and refs are deleted.
6. The session worktree remains on disk for the existing cleanup flow.

## Alternatives and Tradeoffs

### Rejected: extend `WorktreePlugin` to own sync lifecycle

Why rejected:

- the plugin contract in `core/worktree-plugin` is intentionally narrow
- config-loaded third-party plugins should not mutate daemon-owned sync state
- daemon recovery depends on durable Git-owned state, not plugin-local behavior

Tradeoff:

- less abstraction
- clearer semantics and fewer invalid implementations

### Rejected: explicit writer handoff

Why rejected:

- the user can reasonably decide to start syncing after edits already exist on both sides
- explicit handoff complicates the operational model without reducing Git work
- automatic merge with a fixed conflict preference better matches the desired behavior

Tradeoff:

- more Git work per cycle
- less operational ceremony for the user

### Rejected: fail the second mount when a primary checkout is already mounted

Why rejected:

- the user may reasonably activate sync from a newer session without first cleaning up the older mounted session
- the design already requires one repo-scoped lock and reversible unmount
- replacing the old mount preserves the single-mounted-session invariant without introducing extra user-facing ceremony

Tradeoff:

- one session can lose sync ownership implicitly
- the primary checkout still has one unambiguous mounted owner at a time

### Rejected: patch-wins overwrite without merge

Why rejected:

- it discards non-conflicting edits from the other side
- it turns fast automatic sync into hidden last-writer-wins behavior
- `git merge -X ours` keeps more useful work while still giving the worktree a deterministic tie-breaker

Tradeoff:

- scratch merge logic is more complex than plain overwrite
- the result preserves much more work

### Rejected: incremental apply over the destination's current dirty state

Why rejected:

- it compounds drift across repeated syncs
- conflict behavior depends on the exact current state of the destination
- it makes retries and recovery harder to reason about

Tradeoff:

- more Git operations per sync
- deterministic state reconstruction and simpler reasoning

## Failure Modes and Edge Cases

- primary checkout `HEAD` moved away from `baseOid`
  - sync fails and requires unmount or remount through a new base
- session worktree `HEAD` moved away from `baseOid`
  - sync fails and requires unmount because snapshot commits are anchored to the mounted base
- a newer session mounts against the same primary checkout
  - the older mounted session is automatically unmounted and its live sync state disappears without a user-facing error on the newer mount request
- replacement unmount of the older mounted session fails
  - the newer mount fails too, because the daemon must restore the primary checkout to one known state before mounting another sync session
- watcher setup fails or later degrades
  - automatic sync stops for that mounted session, but manual sync and unmount remain allowed
- original primary branch advanced while mounted
  - unmount restores the original commit in detached mode and returns a warning
- merge still leaves unresolved binary, rename, or mode conflicts after `-X ours`
  - the daemon stages the worktree version for those paths, records a warning, and retains the primary recovery snapshot
- sync metadata file exists but refs are partially missing
  - `inspect()` treats the session as corrupted, records diagnostics, and refuses sync until unmount cleanup
- daemon crashes after detaching the primary checkout but before session startup completes
  - reconciliation uses the Git-owned metadata file to attempt unmount
- linked-worktree provisioning fails
  - session creation fails before the agent starts

## Testing, Rollout, and Observability

### `core/daemon/src/worktrees/` tests

- mount preserves and later restores primary pre-mount tracked and untracked state
- manual mount for one already-existing linked session worktree records the worktree `HEAD` as `baseOid`
- mounting a second session against the same primary checkout auto-unmounts the first session before recording the new mount
- sync with only worktree-side changes rebuilds both checkouts from one identical result snapshot
- sync with only primary-side changes rebuilds both checkouts from one identical result snapshot
- sync with non-conflicting changes on both sides preserves both edits
- sync with overlapping textual changes prefers the worktree version
- sync with distinct untracked additions on both sides keeps both new paths
- sync with overlapping untracked additions prefers the worktree version for the same path
- unresolved binary or rename conflicts retain `primary/recovery/latest` and emit warnings
- sync rebuilds both checkouts from `baseOid` instead of incrementally stacking prior mirrored state
- unmount warns and detaches when the original branch tip moved
- corrupted metadata or partially missing refs are surfaced as recoverable sync errors

### `core/daemon` session-manager tests

- `session.create` with `worktree.sync.enabled` provisions one session worktree, mounts sync state, and starts the auto-sync scheduler
- `session.mountWorktreeSync()` mounts sync for one existing linked session worktree
- `session.mountWorktreeSync()` on a second session against the same primary checkout stops the older scheduler and clears the older session's live sync state
- debounced watcher events coalesce into one sync cycle
- watcher degradation records diagnostics and leaves manual sync available
- `session.worktree.get` returns live sync state
- `session.syncWorktree()` forces one immediate sync cycle
- `session.unmountWorktree()` restores the primary checkout and clears live sync state
- `session.shutdown` auto-unmounts before completing
- reconciliation attempts best-effort unmount for abandoned mounted sessions

### Rollout and diagnostics

- phase 1 keeps ordinary `worktree.enabled` semantics unchanged when sync is not enabled
- no store migration is required because mounted sync state lives in Git metadata and refs
- session diagnostics should add dedicated events for:
  - `worktree.sync_mounted`
  - `worktree.sync_replaced`
  - `worktree.sync_requested`
  - `worktree.sync_started`
  - `worktree.sync_completed`
  - `worktree.sync_warning`
  - `worktree.sync_watcher_degraded`
  - `worktree.sync_unmounted`

## Open Questions

No blocking open questions remain on the core sync path.

## Ambiguities and Blockers

- AB-1 - Non-blocking - Watcher debounce and retry policy
  - Affected area: Behavioral Semantics / Observability
  - Issue: The design requires one debounced auto-sync scheduler, but the exact debounce interval and watcher-retry policy are not yet fixed.
  - Why it matters: It affects noise, responsiveness, and diagnostics, but not the correctness of the merge model.
  - Next step: Finalize the scheduler constants alongside the `SessionManager` implementation.

- AB-2 - Non-blocking - Recovery snapshot retention policy
  - Affected area: Persistence / Cleanup
  - Issue: The design retains one primary recovery snapshot whenever both sides contribute dirty inputs, but the exact pruning policy for old retained snapshots is not yet specified.
  - Why it matters: It affects disk usage and operator clarity, but not the correctness of one sync cycle.
  - Next step: Decide whether phase 1 keeps only `primary/recovery/latest` or also writes one diagnostic breadcrumb when a newer cycle replaces it.
