# Boss rotation oracle and observation seams

The pre-compaction rotation guarantee needs an oracle that watches the real
provider and durability boundaries. `waku_boss::rotation_oracle` owns the
observation schema, deterministic fixtures, and the reducer that reports
unsafe admissions, compaction entry, ownership conflicts, duplicate provider
effects, and invalid lifecycle transitions. It does not infer safety from a
rotation request.

## Test-only daemon seams

The current lifecycle does not have a measured admission guard. Until the
implementation workstreams land one, daemon unit tests can install
`RuntimeLifecycles::rotation_observer` and receive these observations:

- `RotationObservedDriver` wraps the live Boss `DriverHandle` under tests
  and records every context-growing prompt, continuation, resume/replay, or
  computer-tool result immediately before it reaches the provider. These
  calls are deliberately reported `Unguarded`; `compact` is reported as
  `ProviderCompaction::Attempted` so it can never masquerade as a successful
  rotation.
- `forward_driver_events` maps actual provider events for Boss principals:
  usage updates become context samples, `TurnStarted`/steer acknowledgements
  become transport evidence, and localized compaction activities become
  compaction started/completed/failed observations.
- `rotate_boss_chat` emits lifecycle, durable-action, and ownership
  observations when the existing journal persists an intent, stages the
  successor, publishes the pointer, fences the predecessor, and commits the
  ownership switch.

These are `#[cfg(test)]` seams: they add no runtime I/O or product behavior.
When workstreams 2–4 add acceptance-aware delivery and the U+G+M<C guard,
they should replace `AdmissionDecision::Unguarded` and `None` generation
transport fields at the same observation points rather than adding a second
source of truth.

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
