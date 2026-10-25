# Boss coordinator readiness contract

Boss tries to avoid provider compaction through best-effort session rotation.
Compaction itself does not revoke authority: canonical instructions are restored
on the next outbound prompt alongside instruction pins and current work state.
An ordinary refresh waits for a settled turn, preserves history and queued input,
then hands coordination to one fresh session. Its provider launches lazily through
normal admission. Transcript notices carry recovery feedback; no refresh toasts.

Startup, driver admission and scoped CLI calls still reject archived, retired,
frozen and pending-ownership sessions. Unreadable continuity also blocks
admission. New rotation intents record `mode: bestEffort`; older intents default
to `strict`. An older compaction checkpoint can switch to best effort only when
its successor delivery is still Prepared, no replacement lease exists and neither
owner has unresolved external effects. Unknown attempts retain their original
correlation and never authorize replay. Prepared packages remain as evidence.

The remaining sections describe the retained strict checkpoint contract, which
ordinary rotation does not claim. Current adapters cannot satisfy its provider
qualification, native receipt, budget and policy requirements.

## Integration boundaries

- `BossService::canonical_boss_instructions` renders the authoritative persona,
  role and planning restrictions without consuming one-shot injection state.
  The identity revision also binds planning scope and resource grants.
- `CoordinatorPackage` binds provider, model, generation, identity, grants and
  scope independently of narrative. Its exact digest includes the package's
  instructions, context, event IDs, work IDs and unresolved-effect references.
  Work IDs enumerate employees, plans, outcomes and deliverables in durable
  Boss state; outcome records retain their handoffs and decisions.
- `spawn_runtime_inner` checks existing ownership and continuity fences before
  launch/resume. Healthy ordinary chat does not require startup-budget evidence.
- `BossOwnershipDriver` permits the active Boss and registered planning sessions
  while they are unarchived and unfenced. Scoped CLI calls retain ordinary
  authorization. Retired credentials and frozen replacements remain blocked;
  compaction alone does not block. No native execution-policy qualification is claimed.
- Healthy human prompts use the normal `PromptSubmitted` and provider-delivery
  path. Ownership-fenced human prompts retain their content and attachments in
  the durable queue; employee results remain queued while fenced. Generic
  turn/error/process events cannot acknowledge, reject or consume continuity. Existing legacy accepted records
  cannot establish exact-package acceptance.

## Exact-package adapter seam

`deliver_coordinator_package` prepares the immutable package before submission.
For a fresh attempt it independently checks the expected budget binding against
adapter observations, provider qualification and execution-policy enforcement.
Every current provider fails this check, even with finite synthetic bounds.
There is no fallback to ordinary `prompt`.

The adapter returns one of:

| Outcome | Durable result |
| --- | --- |
| Confirmed | Matching local attempt plus nonempty provider-native session/request receipt; package becomes accepted |
| DefinitivelyNotAccepted | Matching attempt becomes rejected; another attempt may be considered after fresh validation |
| Unknown | Original submitted attempt remains unresolved; no automatic resend |

Current adapters explicitly decline new exact-package submissions and return
unknown for reconciliation. Local attempt IDs and digests are correlation data,
never provider-native receipts. A reopened submitted attempt queries the same
attempt; missing authoritative reconciliation leaves it unresolved. Submission
and receipt persistence are separate transactions. A failed receipt write leaves
submitted state intact. Receipt acceptance establishes receipt only.

`validate_readiness` checks current authority, native acceptance, referenced
and unresolved effects, policy support and independently bound startup evidence.
It never publishes ownership or activates a successor. No current provider can
pass it. Item 3 must supply atomic lifecycle fencing, current event-position
validation and activation; item 4 owns visible lifecycle status and diagnostics.

## Effects and remaining qualification

`begin_coordinator_effect` persists a logical effect identity before dispatch.
Reusing that identity is denied, including after resolution. Unknown effects
remain durable across restart. Resolution requires an evidence reference and
cannot be overwritten with conflicting evidence. This registry provides a
contract for future dispatch integration; this unit admits no new Boss effects
and does not claim universal exactly-once behavior.

No provider-native package receipt/query, effective budget contract or cross-tool
execution policy is qualified. Live provider checks, daemon restart, rotation
scheduling, pointer switching and recovery orchestration are outside this unit.
The [provider budget contract](boss-provider-budget.md) remains the qualification
source; deterministic fixtures establish fail-closed decisions only.

Focused checks:

```sh
mbx test -p waku-boss --lib
mbx test -p waku-drivers --lib coordinator_delivery
mbx test -p waku-daemon --lib rotation_oracle
mbx check -p waku-daemon -p waku-core
```

## Guarded replacement checkpoint

`ContinuityLedger.frozen_coordinator` binds the predecessor, successor and old
generation before waiting for a running turn to settle. New input and results
remain on the durable predecessor queue. Queue emptiness is not a prerequisite
for freezing or preparing a quiescent successor. Oversized inputs stay intact
in the task store; staging enumerates their event identities rather than
silently consuming them.

A staged successor cannot launch as an ordinary task. Its exact checkpoint binds
current identity, grants, model, generation, work references, queued event IDs
and unresolved effects. Repeated triggers and daemon restarts retain that same
checkpoint. Changed authority or event state blocks rather than overwriting a
submitted package. Generic provider events still cannot settle an unknown
receipt or authorize effect replay.

Before pointer publication, `validate_rotation_successor` requires exact native
acceptance and calls the item-2 readiness validator. It repeats this validation
for recovery of pointers published by older code. Missing adapter evidence
leaves the intent pending and both generations fenced against growth. No new
provider process or capacity slot is acquired while qualification is absent.
Every exposed growth boundary independently calls the item-1 budget validator;
missing observations cannot inherit permission from a previous operation.

This is fail-closed checkpoint/recovery enforcement. It does not supply a
qualified adapter, replacement admission/capacity integration, a successful
ownership activation grant, provider-internal loop interception, or live crash
qualification. All current providers remain paused. Those gates must be closed
before the full item-3 lifecycle or prevention outcome can be called complete.

## Replacement-capacity admission gate

`DriverControl::coordinator_replacement_capacity` defaults to unavailable for
all current adapters. Both a fresh exact-package replacement submission and
pre-publication/recovery validation require a broker observation bound to the
predecessor, successor and complete independently expected startup binding.
Unknown receipt reconciliation remains permitted without acquiring or inventing
a new reservation; it cannot replay the package or activate either owner.

`ReplacementCapacity` distinguishes overlapping provider processes from a
confirmed predecessor release. Overlap counts both slots; releasing the old
slot permits replacement at a cap of one. Occupancy includes the successor and
all other held slots in the same pool. Missing evidence, changed binding, empty
reservation, zero occupancy and exceeding the cap block. The positive fixtures
exercise this accounting contract only, not qualification or actual reservation.

The snapshot gate is now backed by the durable lease contract below. No current
adapter supplies either observation or broker. A numeric snapshot or local ID
still cannot authorize publication. Current employee admission does not share
the new pool; connecting every actual launch path is a qualification prerequisite.

## Durable replacement lease

`replacement_lease::ReplacementBroker` persists a provider/model pool, its
immutable identity and hard cap, ordinary live slots, and replacement leases.
`ContinuityLedger.replacement_leases` stores the acquisition intent before the
broker is touched. Each intent binds broker/lease IDs, predecessor, successor,
runtime, generation, configuration, identity and exact startup package. Fresh
budget-operation IDs may change; they are independently checked at each guard
and do not create a new capacity reservation.

| Boundary | Durable rule |
| --- | --- |
| Acquisition | Persist one Requested intent; reserve and sync the slot; persist Held. A crash between those writes reconciles the same ID. |
| Duplicate trigger | Reuse the intent, lease and original deadline. Never reserve twice or extend expiry. |
| Submission/publication | Hold a broker lock through the operation. The daemon keeps that pin through pointer publication, predecessor fencing and durable ownership commit. |
| Restart | Reopen the same broker identity and reconcile the saved lease. Missing/foreign leases, changed bindings and expiry deny publication. A pointer alone is insufficient. |
| Success | Persist Activated after the ownership commit, then settle the lease. The successor remains counted as an ordinary live slot. |
| Failure | Independently confirm that the exact successor runtime stopped, persist Failed, then release its slot. A timeout or provider error is not stop evidence. |

The broker uses exclusive private staging, file sync, atomic rename and Unix
parent-directory sync. Its lock file survives record replacement. Acquisition
and settlement fail promptly if the pool is pinned elsewhere. Dropping a pin or
losing the daemon never releases a durable slot. Unpinned expiry is checked at
the original 120-second lease deadline and blocks future publication; it does
not free potentially running work. Expiry/revocation cannot race a publication
while its broker pin is held. No automatic renewal or unsafe fallback exists.

`reconcile_committed` repairs death after the journal commit but before lease
settlement. It requires the matching durable Activated rotation, generation and
owner; the daemon also verifies its authoritative Boss pointer. Terminal release
is idempotent. A missing broker or uncertain stop retains capacity and pauses.
Lock order is coordinator operation/task state, broker, then brief continuity
transactions; continuity transactions release their lock before acquiring the
broker. Broker code must not acquire task or coordinator operation locks.

Run `mbx test -p waku-boss --lib replacement_lease` for acquisition and restart
faults, pointer/ownership boundaries, terminal-write failures, stale/foreign
bindings, lease loss/expiry, lock exclusion and hard-cap accounting. These are
trusted future-contract fixtures using disposable local stores. They neither
launch a provider nor prove actual process-death or power-loss behavior.

Item 3 remains incomplete. Every actual provider/employee/alternate launch must
share an authoritative pool before an adapter can supply this broker; none does.
Effective provider-internal checkpoints/startup evidence (item 1), native exact
receipt/policy readiness (item 2), and atomic activation-grant publication remain
gates. The pin covers existing ownership publication and must also enclose any
future grant commit. No positive production activation path is enabled. Existing
durable freeze, pending events and uncertain receipts remain intact.
