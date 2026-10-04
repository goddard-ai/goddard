# Employee status semantics

Recommend one daemon-owned execution snapshot shared by `view` and `roster`,
with lifecycle separate from turn activity. A quiet tool call remains inside a
turn; only an explicit turn boundary means the employee awaits instruction.
The sidebar may use simpler labels, but must preserve that distinction.

## What exists today

- [`BossEmployee`](../../crates/waku-protocol/src/boss.rs) stores `expired`,
  `expiredAt`, and `blocker`, but no execution status.
  [`BossService::handle(View)`](../../crates/waku-core/src/boss.rs) returns that
  document, filtered for employee callers. The daemon delegates `view` to it;
  the CLI serializes the result without computing status.
- [`employee_roster`](../../crates/waku-core/src/boss_context.rs) combines those
  records with daemon session state: expired wins, then a reported blocker;
  Connecting/Working become `working`, Waiting/Background become `finishing`,
  Failed becomes `blocked`, and everything else (including a missing session)
  becomes `idle`. Waiting actually includes permission/input requests;
  Background is a parked, still-open turn. Neither necessarily means finishing.
  Its elapsed time uses `updated_at`, not a status-transition timestamp.
- [`record_boss_event`](../../crates/waku-core/src/daemon.rs) sets Working on
  TurnStarted, Background on TurnParked, Waiting on permission/input requests,
  and Idle/Failed on TurnFinished. Activity/RichActivity updates record tool
  progress without changing status. `active_turn_id()` tests the last turn's
  Running state. `session_active()` merely tests existence and non-archival;
  runtime-map membership likewise cannot establish that a turn is running.
- TurnFinished/ProcessExited triggers `BossService::note_settled`, which
  asynchronously calls `finish_boss_employee`: expire the record, shut down
  the runtime, settle any remaining turn, and clear queued prompts. Thus live
  `idle` after completion is normally a short handoff before expiry, not a
  durable employee waiting room. Prompting revives an expired record and
  clears its blocker. Expired errands retire after one hour; goals are exempt.
  Retirement removes the employee record but preserves the task transcript.
- The [`sidebar`](../../src/app/boss.rs) groups employees using expiry, but its
  row indicator uses client SessionStatus: spinner for Connecting/Working,
  hourglass for Background, alert for Waiting, failure icon for Failed, and
  completion/ambient markers for Idle. Its internal `working` set means
  non-expired, not necessarily an active turn.

Commit `072d6cea` fixes runtime teardown notification: emit ProcessExited before
retiring routing so attached clients release stale driver handles and stop
showing the previous working spinner. Its regression test checks that exit
event. It does **not** introduce tool-wait status, enrich `view`, or change the
roster mapping. Current source does not turn an ordinary long tool call idle
just because output stops; the reported 27-minute case needs runtime evidence
to identify its precise cause. The missing distinction is independently clear.

## Proposed contract

Keep durable lifecycle and ephemeral activity separate; expose a derived
`status` for convenient boss decisions. These are proposed fields and labels.

| Boss `status` | Lifecycle / activity | Meaning | Sidebar label |
| --- | --- | --- | --- |
| `running` | live / running | Active turn generating or doing unclassified work | Working |
| `waitingTool` | live / waitingTool | Active turn waiting on an observed tool/subprocess, including parked background work | Waiting on tool |
| `awaitingInstruction` | live / awaitingInstruction | Turn explicitly settled; no active turn or pending submission | Ready |
| `expired` | expired / none | Runtime released; retained employee can be prompted again | Finished |
| `retired` | retired / none | Employee identity released; transcript remains, employee control unavailable | History only |

Include `turnId`, `runtimeId`, `stateSince`, `lastEventAt`, `queuedCount`, and an
optional `wait { kind, toolCallId, since }`. Keep last-turn outcome and reported
`blocker` separate: failure, waiting on permission/input, and a supervisor
attention request are different facts. Add `starting` for accepted submission
before provider start, and `unknown` for unavailable/reconciling execution
evidence; neither may fall back to Ready. Add `waitingInput` for permission/input,
with its reason and sidebar “Needs input” label. An expired failure says Failed,
not Finished, while retaining `status: expired` and its outcome.

## Ownership and transitions

The daemon's accepted prompt and driver events own this snapshot. Submission
enters starting; TurnStarted enters running; identified tool start/end events
track outstanding calls per turn. Enter waitingTool only when observed work
is holding the turn (or TurnParked explicitly parks it); parallel tools may
coexist with model activity. A yielded shell process remains outstanding while
the agent waits for it. Providers lacking that evidence retain running with
unknown wait detail. Silence, elapsed time, a live process, and generic
incomplete activity cards are insufficient evidence of completion or a lock.

TurnFinished settles activity; queued submission takes precedence over Ready.
Keep today's automatic expiry policy: Ready is the brief settled handoff,
then expired. Do not introduce a persistent idle runtime just to add a label.
ProcessExited/restart reconciles an open turn as interrupted, with a cause;
intentional teardown after a settled turn must preserve its outcome. Revive
enters starting; retirement ends employee control. Reject late events using
runtime/turn identity, and clear outstanding waits at settlement. Persist
lifecycle/outcome; rebuild transient execution state on restart rather than
trusting a saved running label.

Have daemon dispatch enrich `view` with additive execution fields without
persisting a second status authority in BossState. Use that same projection
for roster labels, counts, and true state durations. Preserve existing expiry
fields and caller filtering. Retired entries need not clutter the live roster:
an explicit history lookup/tombstone can report retirement without pretending
that every missing employee is retired. Push snapshot changes to clients;
render only cached data. Sidebar text/icon and keyboard-accessible details
should convey wait reason and duration without requiring animation or hover.

Acceptance checks for a future implementation: a silent foreground tool and
a parked background command remain in-turn; permission/input is distinct;
completion proceeds Ready → expired; a new prompt revives; late old-runtime
events cannot resurrect activity; restart records interruption; errands retire
after one hour while goals retain their records. Run these across provider
event adapters and check matching `view`/roster results. This analysis makes
no runtime or UI changes and does not reproduce the reported lock.
