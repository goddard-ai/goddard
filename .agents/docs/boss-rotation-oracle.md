# Boss rotation oracle and observation seams

The pre-compaction rotation guarantee needs an oracle that watches the real
provider and durability boundaries. `waku_boss::rotation_oracle` owns the
observation schema, deterministic fixtures, and the reducer that reports
unsafe admissions, compaction entry, ownership conflicts, duplicate provider
effects, and invalid lifecycle transitions. It does not infer safety from a
rotation request.

## The ownership fence and continuity ledger

`BossOwnershipDriver` wraps every Boss-chat `DriverHandle` in production —
the last daemon-owned boundary before the provider. It admits a
context-growing call only while the durable pointer still names that
session, the session is unarchived, and no compaction incident fenced it.
`compact` is never forwarded: a manual compact writes a durable refresh
request that the reconciler settles as a rotation, and a detected
provider-side compaction records an incident that fences the generation
and requests the same refresh. An unreadable continuity ledger or a missing
session record denies context growth. This ownership fence does not yet
enforce provider qualification or deny provider-native shell execution.

`boss/continuity.json` is the durable ledger beside the journal: bound
handoff deliveries move `Prepared → Submitted → Accepted`, where only a
recorded `TurnStarted` for a submitted delivery consumes
`pending_provider_context`, after acceptance is persisted. An unrelated
turn-start cannot accept a prepared or rejected package. A rejected or
unconfirmed submission stays pending under the same delivery identifier
and reconciles on retry. The ledger also holds compaction incidents and
the rotation request queue.

All production ledger mutations use `ContinuityLedger::update`, which locks
a stable companion file through read/modify/write. This preserves concurrent
handoff, receipt, refresh and incident updates. Successor publication stops
when preparing its durable delivery fails. These changes do not establish
power-loss durability, acceptance of an exact payload, or activation gating:
the legacy turn-start signal has no bound delivery receipt and the existing
rotation still publishes ownership before provider startup. Those adapter
and lifecycle contracts remain required for the full guarantee.

## Exact-package receipt contract

`ContinuityLedger` also supports an exact-package path for adapter integration.
It is not connected to production provider submission yet. Legacy records and
`TurnStarted` handling retain their existing behavior and establish no stronger
receipt evidence.

`prepare_package` binds a delivery id to a session, generation, canonical
instruction text, identity revision, coordinator/planning scope and continuity
text. Repreparing identical content is idempotent; changed content or identity
is rejected. A legacy record cannot be upgraded into an exact receipt.

The adapter integration sequence is:

1. Persist `prepare_package` and `submit_package` through
   `ContinuityLedger::update`. The returned `ContinuityAttempt` contains the
   delivery, session, generation, runtime, unique attempt and package digest.
   It is correlation data, not acceptance evidence.
2. Deliver the persisted package intact, using the provider's highest-priority
   persistent instruction surface for canonical instructions. The package does
   not yet represent attachment delivery or autonomous tool continuations.
3. On explicit adapter acceptance, persist `accept_package` with the echoed
   correlation. Every field must match. An unbound turn-start cannot accept
   this package, and a stale attempt cannot accept its retry.
4. Reconcile an unconfirmed submission before retrying. `submit_package`
   rejects an already-submitted attempt. Only a correlated definitive rejection
   through `reject_package` permits a new attempt; process exit and ordinary
   turn failure cannot authorize replay of this package.

For example, losing the acceptance response across a daemon restart leaves
`Submitted` and the same attempt on disk. Reconcile that attempt with the
provider; do not generate a second attempt merely because the daemon restarted.
Duplicate accepted receipts are idempotent. Receipt persistence failure leaves
the original attempt unresolved. Loading a package with missing or mismatched
receipt correlation fails closed, including changes to the persisted scope.

Package files contain private instructions and context. Ledger staging uses
exclusive creation, mode 0600 on Unix, and file sync before atomic rename.
Directory/power-loss durability and real process-death qualification are not
established by the deterministic restart tests.

Activation still needs independent validation of current authoritative identity,
scope, grants, event position, pending effects and startup headroom before
coordination ownership transfers. Receipt acceptance does not grant activation.
Production integration requires a provider-correlated receipt, persistent
instruction delivery and provider-native execution policy. The legacy
`DriverControl::prompt` returns unit and `DriverEvent::TurnStarted` has no
correlation fields; those interfaces cannot supply this proof. The opt-in stable
runtime has durable dispatch identities, but that alone does not qualify Boss
instruction delivery or native tool policy. Do not silently route unsupported
Boss providers through it or mark them qualified.

Focused regression command: `mbx test -p waku-boss --lib rotation_continuity`.
This tests durable bindings, stale/foreign receipt rejection, unknown-effect
replay refusal and persistence failure; it does not test actual providers.

## Test-only daemon seams

Daemon unit tests install `RuntimeLifecycles::rotation_observer` and
receive these observations:

- The production `BossOwnershipDriver` reports each admission decision
  (`Unguarded` while the ownership fence holds — the measured U+G+M<C
  guard is still a workstream-3 item — or `Blocked` when it denies) and
  the `Submitted` transport for the pending bound delivery.
  `RotationObservedDriver` keeps the observation-only wrap for other Boss
  principals.
- `forward_driver_events` maps actual provider events for Boss principals:
  usage updates become context samples, `TurnStarted`/steer acknowledgements
  become transport evidence bound to the journal's generation, and
  localized compaction activities become compaction
  started/completed/failed observations.
- `rotate_boss_chat` emits lifecycle, durable-action, and ownership
  observations when the existing journal persists an intent, stages the
  successor, publishes the pointer, fences the predecessor, and commits the
  ownership switch.

The observer seam itself stays `#[cfg(test)]`: it adds no runtime I/O or
product behavior. When the guarded lifecycle lands it should replace
`AdmissionDecision::Unguarded` at the same observation points rather than
adding a second source of truth.

## Fixture coverage

`rotation_oracle::fixtures` provides deterministic traces for:

- the strict boundary (`U + G + M < C`, including equality and one unit on
  either side);
- rejected send, lost acknowledgement, recovery after every modeled handoff
  stage, duplicate triggers, replayed dispatch, an intentionally unsafe
  admission, and provider compaction entry;
- sustained seeded event ordering through `randomized_trace` and
  `shuffled_tail`.

The daemon tests prove the seams observe real driver commands, provider
events, and the existing journal. Current reports intentionally mark Boss
provider admissions unsafe until the guarded lifecycle exists. Integrated
acceptance tests should assert `is_clean()` only after the implementation
supplies guarded bounds, provider acceptance evidence, and successor
activation.
