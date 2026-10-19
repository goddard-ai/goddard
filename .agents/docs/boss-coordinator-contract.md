# Boss coordinator readiness contract

Boss and planning runtimes remain paused for every current provider. Item 2
stages canonical identity and durable references before launch, then calls the
item-1 startup validator with no effective-runtime evidence. It does not infer
versions, configuration, controls or bounds from PATH or saved settings. The
running daemon is unaffected until separately rebuilt and restarted.

## Integration boundaries

- `BossService::canonical_boss_instructions` renders the authoritative persona,
  role and planning restrictions without consuming one-shot injection state.
  The identity revision also binds planning scope and resource grants.
- `CoordinatorPackage` binds provider, model, generation, identity, grants and
  scope independently of narrative. Its exact digest includes the package's
  instructions, context, event IDs, work IDs and unresolved-effect references.
  Work IDs enumerate employees, plans, outcomes and deliverables in durable
  Boss state; outcome records retain their handoffs and decisions.
- `spawn_runtime_inner` stages that package and validates startup before any
  provider process, native session resume or inherited execution surface starts.
  Current effective runtime version/configuration is unknown, so startup fails.
- `BossOwnershipDriver` holds current and stale Boss/planning handles against
  context growth. No committed readiness grant exists in this unit. Scoped
  Boss/planning CLI calls are denied at RPC ingress, including alternate launch
  and script routes. Human controls and employee paths retain their existing
  authorization. Native execution restrictions are not advertised as supported.
- Human prompts retain their full content and attachments in the durable queue;
  employee results remain queued. Generic turn/error/process events cannot
  acknowledge, reject or consume continuity. Existing legacy accepted records
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
