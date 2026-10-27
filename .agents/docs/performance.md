# Performance

- Treat performance as a product requirement, not a follow-up. Goddard is a native
  app competing with web clients, and staying smooth under a long transcript on
  a high-refresh display is the point of being native. Prefer the faster design
  when it costs nothing in clarity, and measure before assuming a cost is fine.
- Never block the UI thread with heavy work. Rendering owns it, so anything a
  frame can reach must already be in memory: no subprocess spawns, no
  filesystem walks, no network, no blocking locks, no synchronous IPC.
- Row builders and measurement paths run for every visible item on every frame.
  Treat I/O reached from `render` as a defect even when it looks cheap, is
  cached after the first hit, or only triggers for some rows — one `git`
  invocation is already several frames of budget.
- Render paths must not call `.update`, `.read`, or `.read_with` on `Waku` or
  any entity already leased above them. Transcript rows render under
  `entity.update` and `WakuPane` content runs inside `waku.update`, so a nested
  lease aborts the process (`double_lease_panic`, no unwind). Helpers that need
  `Waku` state while building elements take `&Waku` or `&mut Context<Waku>`;
  a `&WeakEntity<Waku>` parameter signals deferred use only — callbacks and
  continuations run outside the lease and may update freely.
- Move the work to `cx.background_executor().spawn`, store the result on the
  entity, and `cx.notify()` when it lands. Render then reads only that store,
  and a miss means "not known yet" and must degrade gracefully.
- Resolve a whole session or collection in one background pass instead of
  probing per item, and guard it with a generation counter so a result from a
  superseded pass cannot overwrite newer state.
- Catalog inbox results carry a source/generation token. Check it before UI
  application and acknowledge only after the current snapshot is fully applied.
  Reconnect freshness survives superseded work and errors until that completion;
  it must not be consumed just by taking a snapshot from the inbox.
- A catalog source also owns cancellation for its polling, status and recovery
  waiters. Include `CatalogSource::revoked()` in every channel wait, including
  reconnect waits. Replacing/removing a source or closing its inbox disconnects
  that signal and wakes all receiver clones without waiting for a daemon event.
  This ends idle waits; it does not interrupt an RPC already running.
- `PersistedState::merge_session_catalog` retains a UUID lookup index across
  metadata-only refreshes. Session membership, identity, project assignment or order changes must
  call `note_session_topology_change`; `push_session` does that automatically.
  Changing status/text does not invalidate topology. Keep temporary friend-view
  save filtering from invalidating the cache when the final order is unchanged.
- Desktop catalog session reconciliation prepares UUID ordering on the polling
  worker, then applies one host per pump in round-robin order. Each slice prepares
  at most 2,048 local index entries and performs at most 128 session operations;
  these are operation limits, not guarantees about elapsed time. Ordered indexes
  avoid whole-table resizing during a slice. Carry the UUID cursor and valid local
  index preparation across superseding snapshots so busy hosts still converge.
  Work may advance at the 16 ms background cadence; catalog repaint requests use
  the existing 120 ms streaming cadence, including a final pending repaint.
- Catalog session removals update positional and project-reference indexes with
  `swap_remove`; callers must not depend on storage order. Preserve live history,
  pending metadata and local rows not yet reported by the daemon. A recovered
  incognito row stays protected until an authoritative catalog reports adoption;
  re-registration uses the existing coalesced mandatory-save cadence. Local
  deletion fences clear only after a matching host reports absence in a request
  started after the deletion. Record request start before issuing the RPC.
- Selection fallback scans at most 2,048 sessions per pump and yields to explicit
  selection/project changes. Replaced snapshots, prepared indexes, retired rows
  and buffered inbox contents are reclaimed off the UI thread, including when
  the receiver closes. Keep raw offline-cache snapshots separate from UI overlays.
  Legacy bulk callers and other catalog-triggered UI helpers still require
  auditing; row batching alone does not establish measured desktop responsiveness.
- Project catalogs also prepare at most 2,048 local index entries and perform
  at most 128 project operations per host slice. Task contributions can install
  one referenced project each before their UI effects run, adding at most 128
  dependency upserts. Project/session cursors advance independently, so repeated
  project supersession cannot prevent task progress. Retire absent owned projects
  only after session reconciliation and a valid session-reference index; retain
  unreported local projects until adoption. Project removals use the same
  request-start absence fence as task removals. Keep replaced projects and old
  indexes on the background reclamation path. Direct project membership, identity
  or order changes must call `note_project_topology_change`; use `push_project`
  for local creation.
- Project-name disambiguation collects only UUID/name/path inputs in chunks of
  at most 128 rows per pump, resolves collisions with the shared resolver on a
  worker, then applies at most 128 labels per pump. Membership/order/name/path
  changes revoke old labels through a separate revision. Metadata-only refreshes
  preserve both active name work and derived labels. Direct name/path changes
  must call `note_project_names_change`; desktop callers use
  `refresh_project_names` to invalidate and wake the pump. Only one resolver
  runs at a time, and it wakes the owner on completion rather than requiring UI
  polling. Startup, catalog completion, creation/removal, relocation and host
  removal use this path. Snapshot chunks and abandoned/result storage reclaim
  off the UI thread even when a weak update fails after window closure. Labels
  may briefly show their canonical names while disambiguation catches up; label
  changes use the coalesced catalog repaint cadence.
- Transcript hydration deduplication and selected-task promotion use indexed
  ordering, not scans of all waiting tasks. There are at most two reads in flight.
  Each start attempt invocation examines at most eight candidates, including
  stale/already-loaded rows; remaining ready work resumes through the pump's
  background cadence. Navigation and completion callbacks wake the pump when a
  bounded attempt leaves ready work. Queue indexes reclaim off the UI thread
  on window closure. This bounds queue administration, not detail merge,
  transcript trimming, or the other hydration-completion helpers.
- Wrap plain background detail/attachment results in `BackgroundOwned` before
  crossing back to the UI. Keep that ownership through weak updates and stale
  branches; unwrap only when installing the payload. A disappeared task,
  duplicate attachment or already-loaded transcript must retire incoming data
  on the worker pool. Both detail reads and attachment baselines use the shared
  `merge_hydrated_session` overlay so newer list edits survive; replaced skeletons
  and old runtime payloads also reclaim in the background. GPUI-affine state
  keeps its owning-thread cleanup. Reclamation does not wait for idle admission.
- Detail and runtime-attachment results carry opaque per-attempt authority from
  `PendingSessionRequests`, plus their owning daemon and its request epoch.
  Completion removes only the matching attempt. Task retirement, friend-view
  replacement/removal and host edits/removal revoke stale authority; host
  revocation changes an epoch without scanning all pending requests. Hydration
  keeps revoked reads in its physical two-read accounting until their callbacks
  finish. Requeued tasks get new authority and stale queued work is skipped by
  the bounded drain. Installing a loaded baseline cancels obsolete detail; keep
  a still-needed read if installation leaves only a skeleton. Discard rejected
  payloads through `BackgroundOwned` before any state or error UI is applied.
- Archive and dormant-confirmation previews keep `BackgroundOwned` from the
  worker result through queued/dialog ownership. Abandoned weak updates,
  duplicate confirmations, no-warning results, confirm/dismiss and window close
  retire the plain preview payload off the UI thread; focus/scroll state stays
  affine. Rendering those previews still needs bounded row preparation.
- One-shot user actions such as a click or menu command may work synchronously
  when freshness matters more than latency; frames may not.
- Daemon turn checkpoints register workspace-writer evidence before waiting for
  their capture lock, and retain it through Git capture and durable checkpoint
  persistence. Cleanup must check that evidence even after the provider exits.
  Successful capture confirms completion. Publish owed checkpoint work, keyed
  by canonical workspace/session/turn, before waiting for its lock. Errors before
  spawning Git or after successful Git completion retain that debt without
  inventing an unknown physical writer; only a matching successful retry clears
  it. An error during Git capture still retains unqualified writer evidence per
  workspace. Checkpoint success must never erase unrelated unknown providers.
  These process-local preconditions do not replace destructive workspace
  admission, durable failure recovery, or queued desktop work that has not yet
  reached the daemon.
- Daemon terminals commit a narrow unqualified writer origin immediately before
  the PTY factory can create a child. A storage failure aborts before spawn;
  invalid-directory validation creates no origin. Neither terminal close, EOF,
  shell-leader wait nor catalog deletion clears this observation. Unix directory
  identity distinguishes a reused path. Cleanup checks at most 64 origins and
  defers on overflow, corrupt metadata or an unlocatable/replaced origin. This
  conservative inventory is not positive descendant completion, scalable paged
  admission, legacy-origin recovery or final destructive workspace authority.
- Worktree archive/removal checks use the complete Git worktree root, even
  when the request names a nested project. Canonical session archive refs capture
  that whole root; ordinary turn and safety refs retain project-subtree scope.
  `capture_ref_guarded` and `remove_guarded` resolve their actual effect scope
  before the caller's check and retain its returned guard through publication or
  removal. Checking only the requested nested cwd can miss sibling writers and
  discard sibling changes. These hooks do not themselves provide admission or
  archive-generation authority.
- Daemon archive capture/removal holds a process-local path reservation through
  the filesystem operation. Provider factory admission, checkpoint captures and
  terminal origin publication reject intersecting reservations, including aliases
  and ancestor/descendant paths. Registry locks cover only reservation/evidence
  publication; Git and ZIP work run after releasing them. Failed quiet checks
  release their reservation. Denied checkpoint capture retains its owed work.
  This closes local admission races at those boundaries, not cross-daemon
  coordination, preparation/restore ownership, shared catalog owners or stale
  archive-generation authority; those still need the complete executor contract.
- Keep per-frame work proportional to what is on screen. Long collections are
  virtualized with `list()`, and a row builder must not rebuild whole-session
  state; hoist that to a cache refreshed once per frame.
- Streaming CPU is governed by two cadences — stream commits at ≤ ~8.3 Hz and
  pulse-clock ticks at ≤ 60 Hz (spinners; other pulses stay at ≤ ~30 Hz) —
  and by what one frame can see. Read
  [streaming-performance.md](streaming-performance.md) before touching the event
  pump, the pulse clock (`crates/waku-ui/src/ui/motion.rs`), veils, overlay scrollbars, pane
  caching, or anything else a streaming frame reaches; it also records the
  counter-based measurement playbook that actually finds regressions.

## Isolated desktop measurements

A separate task database is insufficient isolation: daemon settings default to
`~/.goddard/settings.json` even with `GODDARD_DATA_DIR`. Fixture daemons must pass
`--settings-path` explicitly. The flag selects that file; home migration and
provider configuration synchronization still use their normal locations.

On macOS, prepare an owned bundle copy with freshly built binaries and verified
signatures. Keep existing watchers and their bundles untouched. Restrict the
fixture process and descendants from home-directory reads, writes outside the
fixture, and external networking; verify the policy with owned probes before
launching. Sandbox path filters must use canonical paths (`/private/var/...`,
not the `/var/...` symlink). Reserve desktop input for the launch/measurement
phase and stop owned fixture processes afterward. Use window metadata, native
stack samples and counters without screenshots. Record source/binary identity,
workload sizes, contention and coverage limits. An event-loop wait in one sample
cannot establish frame latency or provider behavior under pressure.
