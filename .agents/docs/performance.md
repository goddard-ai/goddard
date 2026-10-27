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
  Project name reconciliation and legacy bulk callers still require separate
  bounding; row batching alone does not bound the entire catalog update or
  establish measured desktop responsiveness.
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
  for local creation. Name resolution still runs as a whole-list completion pass
  and must be moved off the UI thread separately.
- One-shot user actions such as a click or menu command may work synchronously
  when freshness matters more than latency; frames may not.
- Daemon turn checkpoints register workspace-writer evidence before waiting for
  their capture lock, and retain it through Git capture and durable checkpoint
  persistence. Cleanup must check that evidence even after the provider exits.
  Successful capture confirms completion; errors retain one unqualified marker
  per workspace. This process-local precondition does not replace destructive
  workspace admission, durable failure recovery, or queued desktop work that has
  not yet reached the daemon.
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
