# Stable agent runtime — stage 1 host

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

## Not yet in this stage

Provider replacement (new generation machinery), Codex/Devin adapters,
daemon/app reconciliation, agent ingress routing (schema only), resource
broker/credential lease migration, watcher/updater integration, real IPC
transport (the host is in-process; `contract` types are the wire shape),
measured native callback deadlines, non-Unix crash cleanup qualification.
