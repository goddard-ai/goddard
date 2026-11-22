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
- One-shot user actions such as a click or menu command may work synchronously
  when freshness matters more than latency; frames may not.
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

## Daemon request timing

Read `requestPools` in the daemon's `daemon-stats.jsonl` to investigate slow
requests from normal use. Samples are written about once a minute. Look at
`workers`, `queued`, and `oldestWaitingMs` to identify a saturated pool. The
oldest wait includes requests still trying to enter the execution queue.

- `active` lists requests holding workers, with the current phase and elapsed
  time when a phase is instrumented.
- `recentSlow` keeps up to 16 records per pool for admission failures, panics,
  and requests taking at least 250 ms. `admissionMs` measures time to queue
  entry, `queueMs` measures time to worker pickup, and `workerMs` measures time
  holding the worker. Correlate records across samples with boot, pool, and
  sequence. Nested phase times can overlap.
- Resource operations have their own labels, such as `agentResources:status`.
  Broker timers cover authority lock wait, device observation, ledger reads,
  and durable writes. Settings and task-view timers expose their lock waits and
  persistence or hydration phases.

The timing records contain operation names, times, and outcome codes. They
carry no request payloads or error text. A `probeDeferred` sample has fresh
request counters alongside older memory and session data when a daemon lock
was busy.
