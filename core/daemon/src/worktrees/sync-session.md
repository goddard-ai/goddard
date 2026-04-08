# Daemon Worktree Sync Sessions

## Overview

The daemon can already provision isolated session worktrees, but it cannot yet mount one session worktree against the primary checkout and mirror dirty state back and forth in a reversible way.

That gap matters because the current daemon worktree host in [`core/daemon/src/worktrees/index.ts`](./index.ts) supports two materially different checkout topologies:

- a linked Git worktree created with `git worktree add --detach`
- an independent copied workspace created with `fs.cp(...)`

The stash-shaped snapshot design only works for linked worktrees that share one Git common dir. It does not work for copied workspaces or any other topology where refs are independent.

This design introduces a daemon-owned worktree sync session model that:

- treats one session worktree as the authoritative writer by default
- mirrors dirty state into the primary checkout through stash-shaped snapshot commits stored under a Goddard-owned ref namespace
- restores the primary checkout to its exact pre-mount tracked state on unmount
- exposes explicit sync lifecycle control through daemon, schema, and SDK surfaces

This phase intentionally excludes `app/` changes.

## Context

### Current ownership boundaries

- `core/daemon/src/worktrees/`
  - Owns daemon-managed worktree creation and cleanup.
  - `createWorktree()` resolves plugins, provisions one worktree, and returns durable metadata.
- `core/daemon/src/session/worktree.ts`
  - Owns reuse and cleanup of persisted session worktrees.
- `core/daemon/src/session/manager.ts`
  - Owns `resolveLaunchWorktree()` and persists session worktree metadata in `db.worktrees`.
- `core/worktree-plugin`
  - Owns only the shared plugin contract used by built-in and third-party worktree plugins.
- `core/schema` and `core/sdk`
  - Expose read-only `sessionWorktree` lookup, but no sync lifecycle mutations.

### Current mismatch with the generic plan

The generic plan assumed a standalone worktree package that owned Git mechanics directly. That is no longer the current structure.

In the rebased tree:

- daemon-owned Git worktree behavior lives under `core/daemon/src/worktrees/`
- `@goddard-ai/worktree-plugin` is only a shared plugin-types package
- `defaultPlugin.setup()` still prefers copy-on-write workspace copies before falling back to `git worktree add`
- persisted `db.worktrees` metadata does not record whether one session checkout is linked or copied

The design therefore needs to extend the daemon worktree host directly instead of describing a separate `core/worktree` package.

## Goals

- Keep ordinary `worktree.enabled` sessions working exactly as they do today unless sync is explicitly requested.
- Support one reversible mount between the primary checkout and one daemon `Session Worktree`.
- Preserve the primary checkout's pre-mount tracked state and restore it on unmount.
- Default to single-writer semantics with explicit writer handoff.
- Rebuild the destination from `baseOid + latest snapshot` on every sync instead of incrementally applying over prior mirrored state.
- Persist Git-owned sync state outside daemon memory so the daemon can recover and inspect mounted sessions after restart.
- Expose the capability through `core/schema`, `core/daemon`, and `core/sdk` without requiring `app/` work.

## Non-Goals

- Syncing copied workspaces, independent clones, or arbitrary repositories that do not share one Git common dir.
- Automatic merge behavior when both sides drift independently.
- Background polling or auto-sync loops.
- Syncing ignored files.
- Mounting a previously created non-sync session in phase 1.
- Deleting the session worktree itself during unmount.
- Extending the third-party `WorktreePlugin` contract beyond setup and cleanup.

## Assumptions and Constraints

- The daemon session id is the sync session id. No second identifier is introduced.
- The primary checkout is the existing `repoRoot` resolved from `resolveGitRepoRoot(params.request.cwd)`.
- A sync-enabled session must provision a linked worktree. Copy fallback is disabled for that path.
- Git CLI commands remain the integration mechanism. The implementation must use supported plumbing commands such as `git rev-parse`, `git update-ref`, `git stash`, `git reset`, and `git clean` instead of reading `.git` internals directly.
- Root config remains unchanged in phase 1 except for any optional plugin behavior already supported today.
- The repository is pre-alpha. Optional schema additions and small API shape changes are acceptable.

## Terminology

- `Primary Checkout`
  - The checkout from which the daemon session was launched.
  - In current daemon metadata this is `repoRoot`.
- `Session Worktree`
  - The daemon-provisioned checkout stored in `worktreeDir`.
  - The daemon session runs from `effectiveCwd` inside this checkout.
- `Checkout Kind`
  - The provisioning topology for one daemon worktree.
  - Phase 1 values are `linked-worktree` and `copied-workspace`.
- `Linked Worktree`
  - A checkout that shares one Git common dir with another checkout.
  - Only this topology supports sync sessions.
- `Sync Session`
  - The Git-native linkage between one `Primary Checkout` and one `Session Worktree`, keyed by daemon session id.
- `Base OID`
  - The commit OID that both checkouts must continue to reference while the sync session is mounted.
- `Snapshot Commit`
  - A stash-shaped commit that captures dirty tracked state, index state, and optionally untracked files relative to one checkout's `HEAD`.
- `Active Writer`
  - The only side allowed to originate new mirrored edits until an explicit handoff occurs.
- `Sync Mode`
  - Either `tracked-only` or `full-with-untracked`.
- `Diverged Session`
  - A sync session where both sides changed since the last successful handoff or sync. Diverged sessions refuse further sync until unmount.

## Proposed Design

### 1. Ownership

#### `core/daemon/src/worktrees/`

Owns the Git-native mechanics:

- linked-worktree provisioning for sync-enabled sessions
- shared ref namespace creation and cleanup
- metadata file persistence under the Git common dir
- mount, sync, handoff, inspect, and unmount operations
- divergence detection and Git-level recovery behavior

This logic lives outside `WorktreePlugin`. Sync semantics depend on Git common-dir invariants that copied workspaces and arbitrary plugins do not provide.

#### `core/worktree-plugin`

Remains intentionally narrow:

- defines `WorktreePlugin`
- supports setup and cleanup for built-in and third-party worktree strategies
- does not own sync session state, Git ref management, or mirror semantics

#### `core/daemon/src/session/manager.ts`

Owns orchestration:

- requesting linked-worktree provisioning during `resolveLaunchWorktree()`
- mapping daemon session ids to sync sessions
- surfacing sync state through `getWorktree()`
- exposing explicit sync, handoff, and unmount IPC actions
- recording session diagnostics for mount lifecycle events
- auto-unmounting mounted sync sessions during explicit shutdown and reconciliation

#### `core/schema` and `core/sdk`

Own the typed control plane:

- sync-enabled session creation params
- sync, handoff, and unmount request and response shapes
- thin SDK methods that mirror the daemon IPC contract

### 2. Provisioning changes in `core/daemon/src/worktrees/`

`createWorktree()` needs an explicit topology requirement in addition to the existing `branchName`, `requestedCwd`, and plugin inputs.

Proposed public shape:

```ts
type WorktreeTopology = "auto" | "linked-worktree-only"
type CheckoutKind = "linked-worktree" | "copied-workspace"

interface CreateWorktreeOptions {
  cwd: string
  branchName: string
  requestedCwd?: string
  plugins?: WorktreePlugin[]
  defaultPluginDirName?: string
  topology?: WorktreeTopology
}

type CreatedWorktree = {
  repoRoot: string
  requestedCwd: string
  effectiveCwd: string
  worktreeDir: string
  branchName: string
  poweredBy: string
  checkoutKind: CheckoutKind
}
```

Behavior:

- `topology: "auto"` preserves today's behavior.
- `topology: "linked-worktree-only"` requires the created checkout to share one common Git dir with the primary checkout.
- `plugins/default.ts` must skip copy-on-write cloning when `linked-worktree-only` is requested and fail if `git worktree add --detach` cannot create the checkout.
- `plugins/worktrunk.ts` may satisfy `linked-worktree-only` if the resulting checkout resolves to the same common dir; otherwise it returns `null` and the default plugin handles the final attempt.

`db.worktrees` and `DaemonSessionWorktree` both gain `checkoutKind`.

### 3. New daemon-owned sync host

Add one module under `core/daemon/src/worktrees/`, tentatively `sync.ts`, that owns sync session operations.

```ts
type WorktreeSyncMode = "tracked-only" | "full-with-untracked"
type WorktreeSyncWriter = "worktree" | "primary"
type WorktreeSyncStatus = "mounted" | "diverged"

interface WorktreeSyncSessionState {
  sessionId: string
  status: WorktreeSyncStatus
  mode: WorktreeSyncMode
  activeWriter: WorktreeSyncWriter
  primaryDir: string
  worktreeDir: string
  commonDir: string
  baseOid: string
  primaryOriginalHeadOid: string
  primaryOriginalSymbolicRef: string | null
  primaryOriginalBranchTipOid: string | null
  primaryLatestSnapshotOid: string | null
  worktreeLatestSnapshotOid: string | null
  lastAppliedToPrimaryOid: string | null
  lastAppliedToWorktreeOid: string | null
}

class WorktreeSyncSessionHost {
  constructor(input: {
    sessionId: string
    primaryDir: string
    worktreeDir: string
  })

  inspect(): Promise<WorktreeSyncSessionState | null>
  mount(input?: { mode?: WorktreeSyncMode }): Promise<WorktreeSyncSessionState>
  syncToPrimary(): Promise<WorktreeSyncSessionState>
  syncToWorktree(): Promise<WorktreeSyncSessionState>
  handoff(nextWriter: WorktreeSyncWriter): Promise<WorktreeSyncSessionState>
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
refs/goddard/worktree-sync/<session>/last_applied_to_primary
refs/goddard/worktree-sync/<session>/last_applied_to_worktree
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
  "mode": "tracked-only",
  "status": "mounted",
  "activeWriter": "worktree",
  "primaryOriginalHeadOid": "def...",
  "primaryOriginalSymbolicRef": "refs/heads/main",
  "primaryOriginalBranchTipOid": "def...",
  "mountedAt": 1770000000000
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
- `checkoutKind`

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
  mode: z.enum(["tracked-only", "full-with-untracked"]).optional(),
})

const SessionWorktreeParams = z.object({
  enabled: z.boolean().optional(),
  sync: SessionWorktreeSyncParams.optional(),
})
```

Semantics:

- `worktree.enabled: true` with no `sync` block preserves today's isolated-session behavior.
- `worktree.sync.enabled: true` requires `worktree.enabled: true`.
- sync-enabled session creation provisions a linked worktree, mounts the primary checkout before the agent starts, and defaults `mode` to `tracked-only`.

### Persisted session worktree response

Extend `DaemonSessionWorktree` with:

```ts
const DaemonSessionWorktreeSync = z.strictObject({
  status: z.enum(["mounted", "diverged"]),
  mode: z.enum(["tracked-only", "full-with-untracked"]),
  activeWriter: z.enum(["worktree", "primary"]),
  baseOid: z.string(),
  primaryLatestSnapshotOid: z.string().nullable(),
  worktreeLatestSnapshotOid: z.string().nullable(),
  lastAppliedToPrimaryOid: z.string().nullable(),
  lastAppliedToWorktreeOid: z.string().nullable(),
})

const DaemonSessionWorktree = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
  checkoutKind: z.enum(["linked-worktree", "copied-workspace"]),
  sync: DaemonSessionWorktreeSync.nullable(),
})
```

`sessionWorktree` stays the read endpoint, but `SessionManager.getWorktree()` now merges persisted `db.worktrees` metadata with live sync-session state from the Git metadata file when present.

### New daemon control routes and IPC actions

Add three daemon-owned mutations:

- `POST sessions/:id/worktree/sync`
  - syncs the current `activeWriter` into the opposite side
- `POST sessions/:id/worktree/handoff`
  - payload: `{ id, writer: "worktree" | "primary" }`
  - if the current writer has pending changes, the daemon syncs that side first and only then switches `activeWriter`
- `POST sessions/:id/worktree/unmount`
  - removes the sync session and restores the primary checkout

All three routes return:

```ts
type MutateDaemonSessionWorktreeResponse = DaemonSessionIdentity & {
  worktree: DaemonSessionWorktree | null
  warnings: string[]
}
```

The SDK adds thin methods:

- `sdk.session.syncWorktree(...)`
- `sdk.session.handoffWorktree(...)`
- `sdk.session.unmountWorktree(...)`

No `app/` surface is added in this phase.

## Behavioral Semantics

### 1. Mount on sync-enabled session creation

When `worktree.sync.enabled` is requested:

1. `resolveLaunchWorktree()` resolves the primary checkout via `resolveGitRepoRoot(params.request.cwd)`.
2. `createWorktree()` provisions the session worktree with `topology: "linked-worktree-only"`.
3. The daemon verifies that both checkouts share the same Git common dir.
4. Read:
   - `worktreeHead = git -C <worktreeDir> rev-parse HEAD`
   - `primaryHead = git -C <repoRoot> rev-parse HEAD`
   - `primarySymbolicRef = git -C <repoRoot> symbolic-ref -q HEAD || true`
5. Set `baseOid = worktreeHead`.
6. Capture the primary checkout's pre-mount dirty state into `primary/pre_mount`:
   - tracked-only: `git stash create`
   - full-with-untracked: `git stash push -u`, `rev-parse stash@{0}`, `stash apply --index`, `stash drop`
7. Record metadata and initialize shared refs.
8. Move the primary checkout to a detached `HEAD` at `baseOid`.
9. Reset tracked state to `baseOid`.
10. In `full-with-untracked` mode, remove untracked files with `git clean -fd`.
11. Set `activeWriter = worktree`.

The session worktree remains on its existing branch. Only the primary checkout becomes detached while mounted.

### 2. Snapshot capture

#### Tracked-only

Capture tracked and index state with:

```bash
git stash create "goddard:worktree-sync:<session>:<side>"
```

Properties:

- non-invasive on the source checkout
- does not capture untracked files
- absence of an OID means the side is clean for tracked and index state

#### Full-with-untracked

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

### 3. Sync from worktree to primary checkout

`syncToPrimary()` performs:

1. Acquire the repo lock.
2. Verify the sync session exists and `activeWriter == worktree`.
3. Verify both `HEAD`s still equal `baseOid`.
4. Capture the worktree snapshot into `worktree/latest`.
5. Capture the primary checkout snapshot into `primary/latest` for divergence detection only.
6. If both sides changed since the last successful sync or handoff, mark the session `diverged` and fail.
7. Rebuild the primary checkout from `baseOid`:
   - `git reset --hard <baseOid>` for tracked state
   - `git clean -fd` only in `full-with-untracked`
8. Apply the worktree snapshot with `git stash apply --index <oid>` when one exists.
9. Update `last_applied_to_primary` to the nullable snapshot state that was just applied.

If the source side is clean, the destination is rebuilt to the clean `baseOid` state and `last_applied_to_primary` is cleared.

### 4. Sync from primary checkout to worktree

`syncToWorktree()` is symmetric, but it requires `activeWriter == primary`.

The handoff operation is the only supported way to switch to `primary` as the writer:

1. if `activeWriter` already matches the requested writer, return the current state
2. otherwise sync the current writer into the opposite side
3. set `activeWriter` to the requested side

This ensures the writer handoff happens at a known synchronized boundary.

### 5. Divergence detection

The implementation compares nullable snapshot states on both sides:

- a commit OID means that side currently has a captured dirty snapshot
- a missing ref means that side is currently clean

A sync is allowed when at most one side differs from the last applied snapshot state from the opposite side. If both sides differ, the session becomes `diverged`.

While `status == diverged`:

- `sessionWorktree` continues to report the live state
- `sync` and `handoff` fail with `IpcClientError`
- `unmount` remains allowed

### 6. Tracked-only semantics

Tracked-only mode deliberately does not move or restore untracked files.

Consequences:

- mount makes the primary checkout tracked-clean, not necessarily fully clean
- untracked files remain local-only on both sides
- divergence detection ignores untracked files
- if untracked files in the destination block snapshot application, the daemon fails the sync, resets tracked state back to `baseOid`, and reports a sync error

This keeps the default mode less invasive while making the failure mode explicit.

### 7. Unmount

`unmount()` performs:

1. Acquire the repo lock.
2. Refuse new sync operations.
3. Rebuild the primary checkout to a clean `baseOid` state first.
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

- `sessionShutdown()` first attempts `unmount()`
- if unmount succeeds, session shutdown proceeds normally
- if unmount fails, `sessionShutdown()` returns `success: false` and records one diagnostic event
- startup reconciliation attempts best-effort unmount for any persisted session whose sync metadata file still exists but whose live daemon session is gone
- if reconciliation unmount fails, the daemon records diagnostics and marks the session record with an error message describing the manual recovery requirement

This differs from ordinary session-worktree cleanup. The worktree directory still follows the existing explicit cleanup model.

## Architecture and End-to-End Flow

### Create path

1. SDK or another daemon client calls `sessionCreate()` with `worktree.enabled: true` and `worktree.sync.enabled: true`.
2. `SessionManager.resolveLaunchWorktree()` asks `createWorktree()` for a linked worktree.
3. `createWorktree()` persists `checkoutKind: "linked-worktree"` through `db.worktrees`.
4. The daemon constructs `WorktreeSyncSessionHost` with:
   - `sessionId`
   - `primaryDir = repoRoot`
   - `worktreeDir`
5. `mount()` captures pre-mount state, detaches the primary checkout, and writes the sync metadata file and refs.
6. The agent process starts in `effectiveCwd` inside the session worktree.
7. `SessionManager.getWorktree()` reads the persisted worktree record, then overlays live sync-session state from Git.

### Sync path

1. SDK calls `session.syncWorktree({ id })`.
2. The daemon validates that the session has a sync-enabled worktree.
3. The daemon loads the Git-owned sync session host by session id.
4. `syncToPrimary()` or `syncToWorktree()` captures nullable snapshot states, runs divergence checks, rebuilds the destination from `baseOid`, and applies the source snapshot when present.
5. The daemon records one session diagnostic event and returns the updated `DaemonSessionWorktree`.

### Unmount path

1. SDK calls `session.unmountWorktree({ id })` or the daemon triggers unmount during shutdown.
2. The daemon loads the Git-owned sync session host and calls `unmount()`.
3. The primary checkout returns to its original tracked state and original `HEAD` identity when possible.
4. The sync metadata file and refs are deleted.
5. The session worktree remains on disk for the existing cleanup flow.

## Alternatives and Tradeoffs

### Rejected: extend `WorktreePlugin` to own sync lifecycle

Why rejected:

- copied workspaces can satisfy `setup()` today but can never satisfy shared-ref sync semantics
- the plugin contract in `core/worktree-plugin` is intentionally narrow and config-loaded third-party plugins should not mutate daemon-owned sync state
- daemon recovery depends on durable Git-owned state, not plugin-local behavior

Tradeoff:

- less abstraction
- clearer semantics and fewer invalid implementations

### Rejected: incremental apply over the destination's current dirty state

Why rejected:

- it compounds drift across repeated syncs
- conflict behavior depends on the exact current state of the destination
- it makes rollback and retries harder to reason about

Tradeoff:

- more Git operations per sync
- deterministic state reconstruction and simpler reasoning

### Rejected: multi-writer automatic merge

Why rejected:

- stash-shaped snapshot commits are not a safe abstraction for automated uncommitted merges
- divergence is easier to detect than to resolve correctly

Tradeoff:

- stricter operational model
- much lower risk of silent corruption or confusing conflict states

### Rejected: reuse copied-workspace topology for sync

Why rejected:

- copied workspaces do not share refs or a common Git dir
- the snapshot-ref design becomes impossible or misleading once each side owns independent refs

Tradeoff:

- sync requires more intentional provisioning
- existing isolation-only sessions remain untouched

## Failure Modes and Edge Cases

- primary checkout `HEAD` moved away from `baseOid`
  - sync fails and requires unmount or remount through a new session
- session worktree `HEAD` moved away from `baseOid`
  - sync fails and requires unmount because snapshot commits are anchored to the original base
- destination untracked files block tracked-only apply
  - sync fails, tracked state is reset back to `baseOid`, untracked files remain
- original primary branch advanced while mounted
  - unmount restores the original commit in detached mode and returns a warning
- sync metadata file exists but refs are partially missing
  - `inspect()` treats the session as corrupted, records diagnostics, and refuses sync until unmount cleanup
- daemon crashes after detaching the primary checkout but before session startup completes
  - reconciliation uses the Git-owned metadata file to attempt unmount
- linked-worktree provisioning fails
  - session creation fails before the agent starts

## Testing and Observability

### `core/daemon/src/worktrees/` tests

- mount preserves and later restores tracked dirty state in tracked-only mode
- full-with-untracked mode preserves and restores untracked files
- sync rebuilds the destination from `baseOid` instead of incrementally stacking prior mirrored state
- writer handoff syncs the old writer before switching ownership
- divergence detection blocks further sync until unmount
- unmount warns and detaches when the original branch tip moved
- corrupted metadata or partially missing refs are surfaced as recoverable sync errors

### `core/daemon` session-manager tests

- `sessionCreate()` with `worktree.sync.enabled` provisions `checkoutKind: "linked-worktree"`
- `sessionWorktree` returns live sync state
- `session.syncWorktree()` mirrors the active writer
- `session.handoffWorktree()` performs the final sync before ownership changes
- `session.unmountWorktree()` restores the primary checkout and clears live sync state
- `sessionShutdown()` auto-unmounts before completing
- reconciliation attempts best-effort unmount for abandoned mounted sessions

### Observability

Add diagnostic events to `sessionDiagnostics`:

- `worktree.sync_mounted`
- `worktree.sync_completed`
- `worktree.sync_handoff`
- `worktree.sync_diverged`
- `worktree.sync_unmounted`
- `worktree.sync_warning`

These are sufficient for this phase. No new metrics system is required.

## Rollout and Migration

- No migration is required for existing session records. New fields are optional.
- Existing `worktree.enabled` sessions keep their current provisioning behavior.
- Sync-enabled sessions are opt-in through `sessionCreate()`.
- The daemon and SDK can ship the full control plane before any `app/` consumer exists.
- Phase 1 intentionally omits copied-workspace support and mount-after-create support.

## Open Questions

None for phase 1.

## Ambiguities and Blockers

- AB-1 - resolved - Copied workspace incompatibility
  - Affected area: Provisioning / Git topology
  - Issue: The default daemon worktree path may create an independent copied workspace that cannot participate in shared-ref sync.
  - Why it matters: Reusing that topology would make the design incorrect.
  - Next step: Add `topology: "linked-worktree-only"` and persist `checkoutKind`.

- AB-2 - resolved - Mount timing for daemon sessions
  - Affected area: Session launch lifecycle
  - Issue: The generic plan treated mount as a separate command, but the current daemon creates worktrees during `sessionCreate()`.
  - Why it matters: A separate mount step would complicate session launch and recovery without helping phase 1.
  - Next step: Perform mount during sync-enabled `sessionCreate()` and expose only sync, handoff, and unmount as daemon mutations.

- AB-3 - deferred - Mounting an existing non-sync session after creation
  - Affected area: Daemon control plane
  - Issue: The current design does not support converting an already running isolated session into a mounted sync session.
  - Why it matters: This may be useful later, but it requires reprovisioning or stricter topology guarantees at create time.
  - Next step: Revisit only if a client needs post-create mount.
