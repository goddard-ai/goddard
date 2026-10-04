# Summon queue and resource policy

## Decision and inventory

The boss sets user-requested policy; the daemon owns admission and dispatch.
A valid summon at capacity returns an employee id in `queued` state, then starts
when capacity frees. Bosses never need to count employees or poll for a slot.
Limits key on **provider + model**, not provider alone. Optional summon resource
claims use the existing host broker.

This is a proposed design, not implemented behavior. Evidence was read on
2026-10-04 from `/Users/alec/dev/worktrees/goddard/dev`, revision
`c5db8949490fc01e7e6734bd09b92c7058f91631` (**D**). The primary checkout at
`fed5d308` lacks the Boss/resource implementation; this document is its only
change. All file:line references below are relative to D.

| Existing plumbing | Confirmed behavior / change point |
| --- | --- |
| `crates/waku-core/src/daemon.rs:6422–6501` | Summon holds `boss.operation_lock`, prepares an identity/grants, calls `create_agent_task_inner`, records a supervisor transcript marker, returns `Summoned { session_id }`. No capacity gate. |
| `daemon.rs:5730–5980` | Task creation resolves inheritance/auto routing, may create a worktree, persists an adopted assignment, publishes the employee roster, then launches. Split preparation/persistence from launch so queued work starts no runtime or worktree. |
| `crates/waku-protocol/src/boss.rs:99–129,189–206,250–272` | Employee has `expired`/`expired_at`, not an explicit lifecycle enum. BossState holds the roster; summon carries assignment, project, optional provider/model/workspace/baseBranch/grants. |
| `crates/waku-core/src/boss.rs:80–135,624–654,1312–1339` | Atomic `boss.json` writes precede revision notification. Startup marks every non-expired employee interrupted and recovery finishes it. Queued records must be excluded. |
| `daemon.rs:6507–6645` | `setModel` rejects working turns, changes provider/model and discards the old runtime. `prompt` resurrects expired employees; `steer` needs a steerable open turn. All are admission bypasses unless gated. |
| `daemon.rs:6747–6834` | Finish expires the employee, begins runtime shutdown, clears pending messages; clean finishes normally send no prompt. Release must follow actual runtime termination, and dispatch needs its own notification. |
| `crates/waku-core/src/resource_broker.rs:51–110,289–394` | Host-wide file lock and ledger, capacity policy file, atomic resource sets and strict FIFO. Dead holder/daemon cancels waiting reservations; live workloads/devices retain claims after release. |
| `crates/waku-protocol/src/resources.rs:7–103` | ResourceSet uses `native_builds`, `resident_devices`, `desktop_input`, and `exclusive` device names; reservations currently depend on PID/deadline, not durable summons. |
| `src/app/boss.rs:223–265,1509–1555`; `boss.rs:891–908` | Sidebar partitions employees by expired flag; `view` returns BossState with caller filtering. Queued employees would otherwise look working. |
| `src/app/goal_dialog.rs:157,946–954` | Existing goals attach to provider threads; statuses have no pending value. A Goals-page pending row needs a daemon-owned assignment/goal link before a provider thread exists. |

## Admission design

**Policy scope.** Model caps cover all Boss employees in this daemon, including
nested summons and planning-supervisor employees, across projects. Ordinary
human tasks and the boss itself are outside employee caps. Host resource caps
remain shared across daemons/tasks through `Broker::host()`. This does not cap
provider-native subagents or arbitrary commands that bypass the cooperative
resource broker.

Use a durable `BossResourcePolicy` distinct from the existing host
`ResourcePolicy`. Each model rule has `liveLimit` and `hardCap`, with
`0 <= liveLimit <= hardCap`. Recommended interpretation of “devin/swe-2 max 6
live, hard cap 8”: normal admission stops at six; an explicitly authorized
`allowBurst` assignment may use slots seven/eight. No automatic overflow and no
hard-cap bypass. This burst interpretation is a proposal requiring product
agreement; without burst authorization, enforce six. Missing model rules impose
no new model cap; an explicit zero pauses that model's queue.

`setResourcePolicy` is boss/human-only, also bound in boss eval. It atomically
replaces the model rules using `expectedRevision`; an optional host section
updates the broker's policy under its authority lock. Return both effective
policies/revisions. Reject the whole update if validation or persistence fails;
use a durable operation intent/reconciliation when both stores change, and do
not dispatch against a partially applied revision. Employees cannot raise caps.
Lowering caps never kills running work: show over-cap usage and block admissions
until it drains. Raising caps wakes scheduling. Policy is daemon state, not a
constraint the boss must remember in its persona or memory.

**Resolve before counting.** Validate caller/persona/grants, nonempty prompt,
project/workspace/base ref, resource shape, and model/effort selection before
accepting. Resolve inherited/default/auto selection to a canonical provider and
concrete model id, persist it, then count that pair. Never reroute silently to
escape a cap. If the concrete default cannot be determined, reject with an
explicit-model requirement rather than count an ambiguous `default` bucket.
Deferred launch revalidates mutable dependencies such as the project/ref.

**One admission ticket.** Persist an employee, assignment envelope, and monotonic
queue sequence before returning success. Create a minimal managed task shell
before publishing the roster so clients can open the assignment; do not adopt
an active turn yet. Extend the broker with daemon-owned admission tickets that
atomically claim a model slot and the declared ResourceSet. Model claims are
namespaced by stable daemon identity; host claims stay global. An empty resource
set is valid for an admission ticket even though ordinary resource acquire
currently rejects it. Never hold a model slot while waiting for host capacity,
or host resources while waiting for a model slot.

A scheduler runs off UI/provider-forwarder threads. It uses durable tickets,
not a boss-side sleep loop. Wake on finish/expiry/shutdown confirmation,
cancellation, policy or model change, and broker availability; retain a bounded
daemon-side reconciliation tick for external-device/process changes. Do not
hold `operation_lock` while waiting, observing host devices, or launching.
Claim under the authority lock; do expensive launch outside it; settle by
id/generation so concurrent stop/model change cannot launch stale work.

**Ordering.** Strict FIFO per root boss, including its nested supervisors:
only that boss's oldest queued assignment may dispatch. Model changes or added
prompts keep its sequence; resurrection is a new assignment at the tail.
This deliberately permits head-of-line blocking (a device-bound job can delay
later work on another model). Across bosses, arbitrate eligible heads by oldest
sequence/time with a stable id tie-break. Extend broker scheduling so a head
blocked solely on its model does not freeze unrelated host-resource clients;
among model-eligible resource requests, preserve the broker's existing FIFO
starvation protection. Canceling the head immediately exposes the next entry.

**Resource ownership.** A summon declaration reserves its full set for the
assignment lifetime. Pass the reservation identity to the employee launcher
so `resource run` borrows subsets using the existing parent mechanism rather
than double-reserving; expansion uses a separate broker request. Jobs needing
resources only briefly should acquire them inside the job instead of declaring
them at summon. Finish/cancel requests broker release; native workload groups
must exit and owned virtual devices must stop before those claims become free.
Never stop user-owned devices. Retained devices can keep host capacity occupied
after the model slot is released. Declaration/enforcement protects admission
and broker operations; it is not OS isolation against raw subprocess launches.

## Lifecycle, controls, and recovery

Persist `state: queued | dispatching | working | finishing | expired` plus an
assignment generation. `dispatching` and `finishing` are explicit accounting
states; today “finishing” is a race window, not a persisted employee state.

| State / transition | Accounting and behavior |
| --- | --- |
| `queued -> dispatching` | Atomic model/resource grant; persist ticket and launch intent before starting a provider. Queued holds zero capacity. |
| `dispatching -> working` | Worktree/task preparation succeeds and initial prompt is submitted once. Publish dispatch event and notify supervisor. Reserved launch slots count even before a first token. |
| `working -> finishing -> expired` | Turn settles, stop, or failure begins teardown. Keep model slot through shutdown; release exactly once after runtime termination. Host claims follow broker workload/device rules. Expired roster reuse remains available. |
| `queued -> expired` | Stop/cancel removes the ticket and records cancellation without a provider launch or fabricated completed turn. No capacity to release. |
| `dispatching -> expired` | Launch failure/stale generation tears down any partial runtime, releases claims, records a failure, and notifies the supervisor. |
| `expired -> queued` | New prompt creates a fresh generation through admission; never resurrect directly into working. |

For queued employees, `prompt` appends durable instructions after the original
assignment, in submission order; they form the initial dispatch envelope and
survive restart. `steer` fails with `employee_not_running`; it cannot steer a
turn that does not exist. `stop` cancels pending work. `setPermissions` preserves
existing grant rules. During finishing, reject new prompt/steer/model changes
with a retryable state error rather than acknowledge work that finish will
clear; a fresh summon remains available.

`setModel` on queued work validates and replaces its resolved pair, retaining
FIFO sequence and waking scheduling. For an admitted employee, preserve the
current prohibition on changing a working turn. At an eligible idle boundary,
shut down the old runtime, release its old-model slot, update provider context,
and requeue continuation against the **new provider + model** cap. No slot
transfer without admission; return queued status when the destination is full.
Do not mutate an in-flight dispatch: cancel/settle that generation first.

Persist policy and assignments/tickets in versioned Boss state, with task-shell
references and a dispatch-notification outbox. Broker tickets derive from those
records and use stable `(daemonId, sessionId, generation)` keys. Ordinary
PID/deadline-bound reservations cannot serve as the durable queue: startup
recreates pending tickets idempotently and restores sequence, not a new request
at the tail. No CLI acquire timeout expires a queued summon.

Exclude `queued` from `BossService::open`'s interrupted set. Reconcile task/Boss
store partial writes and broker claims before scheduling: recreate missing task
shells, discard orphan uncommitted shells, and never launch a record lacking a
committed assignment. For `dispatching`, consult persisted runtime/turn identity:
resume an established launch or mark it interrupted; do not blindly replay the
prompt. Keep current interrupted-failure treatment for previously working
employees, release only after process reconciliation, then dispatch pending work.
Restart preserves queued work even when its supervisor is unavailable; route
notifications to the root boss using the existing report-target fallback.
Corruption fails closed rather than resetting policy/queue.

## Protocol and surfaces

Sketch only; generated TypeScript, CLI schema, eval bindings and all clients
must follow the Rust protocol definitions. Boss fields use camelCase; embedded
ResourceSet retains its existing snake_case fields.

```json
{
  "type": "setResourcePolicy",
  "expectedRevision": 3,
  "modelLimits": [
    { "provider": "devin", "model": "swe-2", "liveLimit": 6, "hardCap": 8 }
  ],
  "host": { "native_builds": 1, "resident_devices": 1, "desktop_input": 1 }
}
```

Add optional `resources: ResourceSet`, `allowBurst: boolean` (default false),
`goalId: UUID`, and `requestId: UUID` to summon. `requestId` deduplicates a
lost-response retry; conflicting reuse is an error. Existing summon fields stay.

```json
{
  "type": "summoned",
  "sessionId": "<employee UUID>",
  "state": "queued",
  "admission": {
    "provider": "devin", "model": "swe-2",
    "queuePosition": 1,
    "blockedBy": [{ "kind": "modelLimit", "used": 6, "limit": 6 }]
  }
}
```

`Summoned` always means accepted, whether queued or immediately dispatching.
Control responses expose resulting state/admission too. Capacity is a wait
reason, not an RPC error; malformed, unauthorized, or impossible requests remain
errors (for example a resource set larger than total host capacity at submission).
If policy later makes an accepted request impossible, retain it pending with a
clear reason until edited, canceled, or policy changes.

BossState/view/context expose lifecycle, queuedAt/sequence, resolved selection,
resources, goalId, queue position, wait reasons, and per-model usage/limits.
Keep prompts/grants behind existing caller filtering. For migration, derive
state from legacy expired/session/runtime data; retain `expired` as a temporary
wire compatibility projection, then upgrade clients before retiring it.

Publish `EmployeeDispatched { sessionId, generation, provider, model, goalId }`
and a durable hidden supervisor notification, independent of `alwaysReport`.
Use an outbox/event id for deduplication across restart; UI listeners receive
revision updates, and a disconnected boss sees the event when it returns.
The summon transcript card updates from pending to working using the same id.
Sidebar has a Pending group with queue position and explicit wait text; opening
a queued employee shows its assignment and cancellation control. Labels and
keyboard actions convey status without requiring hover, color, or animation.
All render data comes from cached daemon snapshots.

**Goals interplay.** A linked queued assignment is pending work on the Goals
page, not active execution, completion, blockage, or a paused provider goal.
Use `executionState: pending | running | finished` on the daemon-owned goal/work
projection, separate from ThreadGoalStatus. Before a provider thread exists,
show the objective, employee, model and admission reason from the persisted
assignment. Dispatch attaches the provider goal/thread to the same goalId and
moves pending to running; waiting consumes no execution budget/time. Pausing or
canceling a pending goal cancels admission; resume requeues at the tail. This
Goals-page integration is proposed: the cited D code provides thread goals,
not this pending-work projection.

## Phased plan and acceptance

1. **Durable queue and policy:** split task preparation from launch; add
   versioned lifecycle/assignment/policy records, boss op/schema/eval bindings,
   model resolution and idempotency. Gate summon, nested summons, resurrection,
   model changes and every managed-runtime launch entry point.
2. **Admission broker and recovery:** add atomic model/resource tickets,
   per-boss FIFO, wakeups, release/teardown reconciliation and notification
   outbox. Preserve current resource-run nesting and retained-device safety.
3. **Clients and Goals:** update generated types, view/context, transcript cards,
   Pending sidebar rows and Goals projection. Ship visible pending state with
   queue activation, not as a later polish step.

Implementation acceptance checks: concurrent summons at limit six never start
seven normally or nine with authorized burst; separate models have separate
counts; destination-full setModel and expired-employee prompt queue; blocked
resource sets claim neither partial resources nor model slots; finish/cancel
unblocks FIFO exactly once; live teardown/device retention never grants too
early; restart and lost RPC responses neither drop nor duplicate assignments
or dispatch notifications; pending Goals rows survive restart and become running
on dispatch. Verify launch failures, lowered caps, missing projects/refs, nested
permissions, and queued prompt/steer/stop behavior. Add executable Test-Plan
trailers to implementation commits; this design-only commit changes no runtime.
