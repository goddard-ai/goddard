# Stable agent runtime — host and daemon reconciliation

`crates/waku-runtime` is the stable provider host: a third, separately
surviving owner for provider sessions per the approved
`plans/stable-agent-runtime.md` and Xavier's stage-0 contract. This page is
the orientation note for the owners who consume its seam — Codex, Devin,
daemon/app reconciliation, watcher/updater.

## What landed

- `waku-runtime::contract` — the typed protocol every owner shares:
  `ActionEnvelope`/`CanonicalAction`/`CommandDisposition`, ordered
  `EventEnvelope`/`EventCursor`, `SessionGrant`/`OfflineMatrix` schema, and
  `HostError`. Nobody invents a parallel envelope.
- `waku-runtime::host::RuntimeHost` — namespace singleton (OS-held lock),
  controller credentials + exclusive control epochs, durable command
  deduplication with conservative uncertain acceptance, ordered durable
  event capture/replay, stop/cancel/drain fences, bounded budgets with
  protected control capacity, crash-vs-clean recovery.
- `waku-runtime::journal` — framed append-only record log (`len|crc32|json`),
  corrupt-tail truncation, snapshot compaction.
- `waku-runtime::adapter` — the provider-ownership seam: `AdapterFactory`,
  `AdapterHandle`, `SessionContext` (bounded intake channel),
  `DispatchOutcome`/`CancelOutcome`/`Termination`, `OfflineMatrix`.
- `waku-runtime::fake` — the deterministic harness: `FakeFactory`,
  `FakeAdapter` (scripted dispatch/cancel/terminate outcomes, `emit`,
  `kill`, `dispatched`), `ManualClock`, `journal_fault` injection.
- `waku-runtime::wire`/`ipc`/`server`/`client` — the private transport:
  length-prefixed JSON over a same-user-verified Unix socket, roles
  (controller, epoch-bound service, emergency, session-scoped agent),
  request/response with long-poll waits, and `endpoint.json` discovery
  inside the owner-only namespace root. `waku-runtime-host` is the
  dedicated executable; `registry` maps admission adapter names to
  factories.
- Scoped ingress and claims — `SessionGrant` (journaled token, immutable
  expiry, constant-time check), deduplicated `IngressRequest`/
  `IngressRecord` with `Pending`/`Forwarded`/terminal-or-`Uncertain`
  dispositions, service drain/resolve for the current daemon, and
  `ResourceClaim`s that survive controller replacement and release only
  explicitly.
- `waku-daemon::stable_runtime` — the opt-in `StableLink`: a
  `DriverControl` over IPC that attaches (never resumes) a task to a
  surviving host session, pumps replay into the forwarder from the durable
  `stable_runtime_links.applied_seq` cursor, acks only after the forwarder
  applies, and answers forwarded agent requests from durable
  `stable_runtime_receipts` before executing them. Opt-in is
  `WAKU_STABLE_RUNTIME_ROOT`/`WAKU_STABLE_RUNTIME_PROVIDERS` (and
  `WAKU_RUNTIME_HOST_BIN` to locate the host executable); without them the
  legacy in-process drivers own providers until stage 4.

The crate has **no `waku-*` dependencies** by design: nothing in its
transitive inputs is product, locale, UI, daemon, or CLI code, so ordinary
product edits cannot rebuild it. Keep it that way.

## Semantics owners must rely on

- **Intent before contact.** Commands journal before dispatch, fences
  before termination, events before publish. `dispatch` is called only
  after `DispatchAttempted` is durable.
- **Epoch fencing.** `acquire_control` atomically invalidates the previous
  epoch. New work under a stale epoch is rejected; recorded commands return
  their stored truth to any live controller credential (dedup precedes the
  epoch fence deliberately).
- **Conservative uncertainty.** `DispatchOutcome::Unknown` becomes
  `Uncertain { AcceptanceUnknown }`; never resent automatically. Crash
  recovery maps `DispatchAttempted` → `DispatchUnknown`,
  `ProviderAccepted` → `OutcomeUnknown`.
- **Single order.** Sequence is assigned at journal time, monotonic per
  generation, identical for replay and live reads. `EventCursor` below the
  retained floor is `CursorExpired` (blocked recovery), never silent loss.
- **Honest stop.** `Stopping` until termination is confirmed; an
  unconfirmed terminate stays visibly stopping and the command is
  `Uncertain { OutcomeUnknown }`. Interrupt (backpressure/crash/drain-forced)
  is reported as `Interrupted`, distinct from a clean `Terminated`.
- **Held dispatch.** `CancelTurn` records a hold; queued submits become
  `DispatchPending` until `PermitDispatch` — the controller's explicit
  policy release, never implicit unwind.
- **Tools/approvals.** `ResolveRequest` resolves once under current
  authority and original request identity. Answer delivery
  (`RequestDisposition`) and provider consumption (`RequestAnswered`
  events) are separate facts; adapter evidence reconciles uncertain
  deliveries without resending.

- **Ingress truth.** Requests are durable before forwarding; a `Pending`
  deadline miss is `Expired`, a `Forwarded` one is `Uncertain` — never
  resent. The daemon dedups effects by `request_id` receipt before
  executing, and records the outcome before reporting it.
- **Attach, not resume.** `start_link` reconciles the host inventory
  before admitting; a live recorded session reattaches under the new
  epoch with its event cursor, pending requests, and claims intact.

## Verified by deterministic tests (`mbx test -p waku-runtime`)

Daemon detach/reattach mid-stream with exactly-once replay; 25-epoch
reload storm with zero duplicate dispatch; stale-epoch fencing; same-id
dedup and payload-mismatch rejection; ambiguous-acceptance hold; stop
fences and unconfirmed termination; cancel-hold + permit; crash vs clean
drain distinction (survivors fenced, commands uncertain, no replay, no
adapter respawn); budget interruption vs pausable backpressure; oversized
message drop markers; disk-failure bounded buffering + recovery; corrupt
tail; emergency stop without a controller; incompatible-hello preserving
sessions; singleton lock; retry-window expiry + clock-rollback floor;
pending-request single resolution and evidence reconciliation; journal
compaction preserving pending requests and requested config.

Daemon-side coverage (`mbx test -p waku-daemon stable_runtime`): link
attach/reattach over IPC, applied-marker ack boundaries, replay of only
unapplied events, receipt-deduped ingress effects, agent-grant ingress
over real sockets, loud failure on unregistered adapters, fenced-epoch
surfacing, wholesale daemon replacement over one `app.db` reattaching to
the surviving session, and the env opt-in gate.

## Not yet in this stage

Provider replacement (new generation machinery), Codex/Devin adapters,
real CLI/MCP/callback binaries speaking ingress (the contract and grant
machinery are landed; `goddard-agent` still uses daemon-address
credentials), resource broker/credential lease migration onto claims,
watcher/updater integration and pinned-artifact isolation, truthful
reconnect surface states in the app projections, measured native callback
deadlines, non-Unix crash cleanup qualification.
