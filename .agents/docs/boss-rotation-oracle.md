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
and requests the same refresh. Unqualified provider configurations fail
closed through the same path.

`boss/continuity.json` is the durable ledger beside the journal: bound
handoff deliveries move `Prepared → Submitted → Accepted`, where only a
recorded `TurnStarted` consumes `pending_provider_context`. A rejected or
unconfirmed submission stays pending under the same delivery identifier
and reconciles on retry. The ledger also holds compaction incidents and
the rotation request queue.

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
