# Goals panel design

Proposal only. Source reviewed on 2026-10-04 at `462b23cc`; no runtime or UI
changes accompany this document. The follow-up implementation should make the
Boss chat's Goals tab an overview of **what was assigned, what is happening,
and which task to open**.

Recommend objective-led rows under **In progress**, **Pending**, and
**Finished**, followed by a compact **Errands** disclosure. Keep projects as
row metadata. Add named wave groups only when their product contract exists.
This keeps ongoing goals readable at the panel's 280px minimum width without
turning the panel into another employee roster (`RIGHT_PANEL_MIN_WIDTH` in
[src/app.rs](../../src/app.rs), `142`).

## Current surface and data boundaries

- [`BossUi`, `BossGoalRow`, and `drain_boss_events`](../../src/app/boss.rs)
  (`44–155`, `218–329`) cache every employee from each daemon's BossState.
  Rows carry employee name, job title, work kind, expiry, and a blocker boolean.
  The cache loses the admission lifecycle, objective, project, and timestamps.
- [`render_boss_goals_panel` / `render_boss_goal_row`](../../src/app/right_panel.rs)
  (`8253–8409`) render a virtualized list with 12px outer padding and a 64px
  initial height estimate. Each raised card says `Name · Job title`, then
  `Goal/Errand · In progress/Completed · Status`. Rows have no navigation or
  keyboard activation. Session lookup scans the session collection per row.
- The panel belongs to the current Boss's **daemon**, not the selected project.
  [`boss_chat_key`](../../src/app/boss.rs) (`598–607`) and
  [`right_panel_owner_allows`](../../src/app/right_panel.rs) (`2367–2395`) define
  that scope. Preserve the existing tab strip and ownership gate. Opening an
  employee task leaves the Boss surface; returning should restore it.
- [`EmployeeGoal`](../../crates/waku-protocol/src/boss.rs) (`99–109`) is a
  summon-time work kind: errands deliver their finish to the supervisor; goals
  normally finish silently and keep their roster record beyond the errand
  retirement window. It is independent of a provider's `ThreadGoal`.
  A Goal row can exist without a thread goal; an Errand can have one.
- Queue plumbing already exists in this revision:
  [`EmployeeLifecycle`, `SummonTicket`, `BossEmployee`](../../crates/waku-protocol/src/boss.rs)
  (`120–239`, `292–372`) carry queued/dispatching/working/finishing/expired,
  admission reasons, sequence, generation, project path, optional goalId and
  groupId. The Goals UI does not use them. [Summon queue](summon-queue.md) is
  earlier design context, not proof of what remains unimplemented.
- `ThreadGoalStatus::BudgetLimited` is terminal, but is not successful
  completion ([model.rs](../../crates/waku-protocol/src/model.rs), `1452–1470`).
  Likewise, expiry alone cannot prove that an objective was achieved. Replace
  the current categorical “Completed” with the neutral “Finished” section.

## Layout and hierarchy

At roughly 340px, two active goals and one queued goal should read as follows.
The boxes show hit regions and alignment, not borders around every row.

```text
┌────────────────────────────────────┐
│ Goals                         +    │  Existing tab strip
├────────────────────────────────────┤
│ 2 in progress · 1 pending           │  Fixed 28px summary; goals only
│                                    │
│ In progress                      2 │  Section header
│ ┌────────────────────────────────┐ │
│ │ Validate the new release flow  │ │  Objective, at most two lines
│ │ Rowan · Goddard                │ │  Employee · project
│ │ ! Needs input                  │ │  Icon + explicit state
│ │ Confirm the signing identity   │ │  Reason when action is needed
│ └────────────────────────────────┘ │
│ ┌────────────────────────────────┐ │
│ │ Fix queued employee dispatch   │ │
│ │ Nina · Goddard                 │ │
│ │ ◌ Working                      │ │
│ └────────────────────────────────┘ │
│                                    │
│ Pending                          1 │
│ ┌────────────────────────────────┐ │
│ │ Audit keyboard navigation      │ │
│ │ Leo · Goddard                  │ │
│ │ ◷ Queued · #1                  │ │  Authoritative queue position
│ │ Waiting for swe-2 · 6/6 slots   │ │  Admission explanation
│ └────────────────────────────────┘ │
│                                    │
│ > Finished                       8 │  Collapsed initially
│ > Errands              2 ongoing   │  Separate, compact disclosure
└────────────────────────────────────┘
```

Goals use one shared list surface: normal rows have no persistent card
background. A hover/focus background reveals the rounded hit region, as in
employee sidebar rows and Automations. This makes spacing and type carry the
hierarchy rather than a stack of equally prominent cards.

The summary counts goals in In progress and Pending, even when those sections
are folded. It stays visible while scrolling. Omit zero phrases; if only
finished goals remain, say `8 finished`. If there are no Goal records, omit the
summary and use the empty treatment below. Do not duplicate the tab's title or
Boss identity in a second large heading.

In progress and Pending start expanded; Finished starts collapsed. With only
finished goals, Finished starts expanded. Honor the user's disclosure choices
after that initial mount; updates do not force sections open. Suppress empty
sections. Each disclosure has an always-visible chevron and numeric count.
Within a folded section, show `1 needs attention` when relevant, so a failure or
reportBlocker cannot disappear behind a quiet count.

### Row anatomy and density

| Element | Proposed treatment | Existing surface to echo |
| --- | --- | --- |
| Panel body | 12px horizontal inset; one scroll viewport; no extra frame | Current [Goals panel](../../src/app/right_panel.rs), `8253–8306` |
| Section header | 28px target; 12px medium text; count tertiary; 12px gap before the next section | [Sidebar group header](../../src/app/sidebar.rs), `4620–4730`: quiet label, disclosure, hover and focus |
| Goal objective | 12.5px medium, `theme.text`, 16px line height, two-line clamp | [Right-panel chooser card](../../src/app/right_panel.rs), `5308–5363`: restrained title, wrapped supporting text |
| Owner/project line | 11.5px, `theme.text_secondary`; single line, truncate project first | [Employee row / boss_sidebar_label](../../src/app/boss.rs), `1588–1652`, `2964–3013`: identity with subordinate detail |
| Status line | 11.5px; 12px icon plus text; reserve the label's space | [Session status indicator](../../src/app/sidebar.rs), `5769–5833`, and [goal_status_color](../../src/app/goal_dialog.rs), `958–967` |
| Reason | Additional 11.5px line, at most two lines; warning tint only for execution problems | [Automation run row](../../src/app/automations.rs), `1280–1387`: concrete failure/precheck detail and task destination |
| Hit region | Full row, 8px radius, 8px horizontal/vertical padding; `overlay` on hover, `focus_highlight()` on focus | [Automation row](../../src/app/automations.rs), `1147–1277` |

A short normal goal is about 64px tall; a wrapped objective adds 16px, and a
reason adds 16–32px. These are sizing targets, not uniform fixed heights.
Use the list's measured heights and invalidate them when width or row content
changes. At 280px, objective, ownership, and state remain stacked; there is no
right-hand badge column stealing title width. At larger widths retain this
anatomy rather than introducing another layout mode.

Do not show the job title alongside every objective: it repeats role information
and crowds out the assignment. Keep it in the full-text tooltip with the
employee name. Avatars remain on the employee's existing sidebar/task identity;
the Goals list uses the status icon as its only small visual anchor. This
distinguishes assigned work from the roster without adding a new icon system.

### Projects and waves

**Status first, project on each row.** This panel answers “what is happening?”
across the current Boss daemon. Project-first nesting would scatter pending and
attention items and consume vertical space with repeated headers. Always show
the human project name, including when only one project is present, to keep
orientation stable as assignments arrive. Resolve from the cached session's
project; for a queued task use the ticket's project reference against cached
projects. Unknown project says `Project unavailable`; never probe the filesystem
from a row. Keep same-name projects distinguishable using their existing
project catalog detail in the tooltip.

**Waves later.** `SummonTicket.group_id` is currently a reserved opaque grouping
field; it does not supply a display name, lifecycle, or membership contract.
Do not infer waves from summon timestamps, supervisor id, job title, or adjacent
rows. For now retain groupId in the prepared data without displaying a UUID.

When named waves land, add one quiet wave subheader inside each state section
only if that state contains multiple waves. A wave may therefore appear under
both In progress and Pending. Keep queue ordering authoritative: contiguous
pending rows may share a wave label, but never reorder the queue to collect a
wave's members together. Single-member/single-wave sections keep wave context
as a metadata line. Unassigned rows remain plain rows.

```text
In progress                         3
  Release validation                2   Named wave, no raised container
    Verify signing …
    Check upgrade path …
  Accessibility pass                1
    Audit task navigation …
Pending                             2
  #1 Audit task navigation …            Queue order survives grouping
  #2 Check rollback …
```

This is a future extension, not part of the first implementation. It requires
a daemon-supplied wave id/name and stable membership, including after dispatch
and expiry. Do not aggregate several employees into one goal row merely because
their tickets share goalId; that relationship does not yet define task ownership
or completion aggregation.

## States and transitions

Prepare a single coarse execution bucket plus a specific status and optional
reason. Call the `running` bucket **In progress** in the UI: it can include an
unfinished paused goal without claiming it is currently executing.

| Evidence, in precedence order | Bucket / visible status | Meaning |
| --- | --- | --- |
| Current lifecycle Queued | pending / `Queued · #n` | Accepted assignment awaiting admission; inspect this before old thread state |
| Dispatching | running / `Starting` | Capacity granted; launching, not a waiting ticket |
| Expired with explicit failure/cancellation outcome | finished / `Failed` or `Canceled` | Work ended; do not use a success checkmark |
| Expired with blocker and no stronger outcome | finished / `Attention requested` | Runtime ended with a report to the supervisor; show its reason |
| Expired with confirmed goal completion | finished / `Complete` | Positive completion evidence for this assignment |
| Expired with a known current budget result | finished / `Budget reached` | Preserve terminal budget exhaustion after teardown |
| Expired without outcome evidence | finished / `Finished` | Neutral legacy fallback, not proof of success |
| Non-expired, current session failed | running / `Failed` | Attention item while lifecycle teardown catches up |
| Current thread goal Complete / BudgetLimited | finished / `Complete` / `Budget reached` | Separate successful completion from terminal budget exhaustion |
| Finishing, without a known terminal result | running / `Finishing` | Teardown in progress; do not speculate about the result |
| Current blocker / thread Blocked | running / `Attention requested` / `Blocked` | Execution issue; reason remains visible |
| Thread Paused / UsageLimited | running / `Paused` / `Usage limit` | Unfinished and resumable; not queue admission |
| Session Waiting | running / `Needs input` | Permission/input request, not waiting for a model slot |
| Session Connecting / Working / Background | running / `Starting` / `Working` / `Background work` | Existing session evidence; Background is an open parked turn |
| Current thread goal Active, with no live turn | running / `Active` | Unfinished pursuit; no evidence that a turn is executing now |
| Non-expired with no usable activity evidence | running / `Status unavailable` | Never infer completion or idle readiness from absent data |

Current outcome storage cannot reliably identify a queued cancellation or
unqualified successful expiry. Without an explicit assignment outcome or
confirmed current thread result, use **Finished**, not Canceled/Complete.
A known budget result must also remain
`Budget reached` after expiry; neutral expiry must not erase a known result.
Provider status belongs to the current assignment generation only. A revived
employee may still have its previous thread goal: that snapshot cannot supply
the new assignment's title, completion, usage, or failure.

```text
Pending (Queued) ── admission ──> In progress (Starting → Working)
     │                                  │
     │ cancel                            ├─ pause / input / limit: stays unfinished
     ▼                                  │
Finished (Canceled if known) <── settle / terminal result / teardown
     │
     └─ new assignment / revival ──> Pending, new generation
```

Sort In progress with actionable input/blockers/failures first, then by
assignment creation time, oldest first, with stable id tie-breaks. Do not sort
by token output or `updated_at`: streaming must not move rows. Pending follows
daemon admission sequence, never project or model. Finished is newest finish
first; missing timestamps sort last. Within the current assignment generation,
Pending → Starting → Working updates the same row, not duplicate rows.

Pending reasons stay neutral: `Waiting for swe-2 · 6/6 slots`, or a sanitized
host-resource explanation. If several resources block admission, show the
first concise reason plus `+1 reason`, with all reasons in the full-text
tooltip. A later FIFO ticket with no recorded blocker says `Waiting for earlier
work`; #1 without a known reason says `Waiting for admission`. Never infer an
ETA, progress percentage, time spent executing, or stalled state from queue age.
Use `Queued` without a number when authoritative position is unavailable.

For glyphs reuse loader-circle, hourglass, alert, x-bold, pause, and an existing
check glyph. Complete may use `theme.success`; budget/input/blockage use
`theme.warning`; queue/paused/neutral finish use secondary text. Pair every icon
with the label. Animate only actual Starting/Working indicators through the
existing motion helper; waiting for capacity needs no spinner. Follow
[accessibility](accessibility.md) and reduce-motion settings.

## Navigation and errands

The **whole goal row opens its employee task** using
`request_session_activation(session_id, SessionActivationTransition::Visit, cx)`,
as [employee sidebar rows](../../src/app/boss.rs) do (`1588–1652`). Employee text
names that destination; it is not a second link to the same place. Give the row
an `Open task: <objective>, <employee>, <status>` label and tooltip containing
the full objective, job title, project, and reason. Hover is supplementary:
the task transcript/goal dialog must expose full text after keyboard activation.
Leave back/forward navigation and the Boss's panel ownership intact.

A queued row opens the assignment's persisted task shell, even before a provider
thread exists. Do not start or resurrect work as a side effect of viewing it.
The current shell contains session metadata but no assignment message
([create_employee_shell](../../crates/waku-core/src/daemon.rs), `8138–8165`).
As part of the queue integration, the task surface needs an assignment preview
from its cached ticket, with full assignment text and admission reason; opening
a blank chat would not fulfill this navigation contract.
If the task cannot be resolved, retain the row and show `Task unavailable`
inline; disable activation until the destination is known. The implementation
must verify remote and unloaded session activation, rather than mistaking a
locally absent session for deleted work. The panel does not need a second
detail view or inline Pause/Resume/Stop controls in its first version: existing
task/goal controls own those operations. Pending cancellation must use employee
admission control, not thread-goal pause.

Errands remain visible for coordination, but get their own disclosure after
goals. In it, use plain **two-line 42–56px rows**, with a one-line task title and
`Employee · State` beneath. Show project as an extra metadata line only when
needed to disambiguate. Reuse the same row activation, keyboard behavior, and
explicit queue status. No persistent raised background and no “Errand” badge
per row; the section already says what these entries are.

The Errands header shows separate `ongoing` and `finished` counts; its contents
order in-progress, queued in FIFO order, then finished. It starts collapsed
when goals exist, expanded when only errands exist. Show an attention count
when folded. Do not include errands in goal totals, merge them with finished
goals, or keep their records forever for this panel: existing retirement/history
semantics remain unchanged. Goals differ through objective wrapping, explicit
state sections, and durable finished history, not brighter color or ornament.

## Empty and unavailable states

Use the [right-panel empty message](../../src/app/right_panel.rs) (`8667–8699`)
title/body typography: 13px medium title, 12.5px secondary explanation, 17px
line height, bounded centered text. Fit it within 12px panel padding. No mascot,
large illustration, sample goals, celebratory completion message, or “Add goal”
control with no existing Boss summon workflow behind it.

```text
No goals yet
Ask your Boss to assign a goal.
Goals stay here after they finish.

> Errands                 2 ongoing     Optional; expands when it is all there is
```

This message applies only to an authoritative snapshot containing no Goal
records. It still permits active Errands below. When only finished goals remain,
show their expanded section rather than an empty message. When the Boss snapshot
has not arrived, show `Loading goals…`; when unavailable, show `Goals unavailable`
with the existing host connection explanation. Retain a previous snapshot when
available and label it `Offline · last known state`; disconnection must not turn
running rows into finished rows or an empty list. Do not add per-row reconnect
buttons; use the app's existing host connection affordance.

## Implementation outline

1. **Prepare display data at snapshot boundaries.** Extend `BossGoalRow` and
   preparation in `drain_boss_events` (`src/app/boss.rs`); refresh when relevant
   session/project/goal snapshots change as well as BossState. Build a session-id
   lookup once per refresh. Preserve `DaemonKey` ownership and stale-revision
   protection. Render only cached, immutable data; see [performance](performance.md).

   The proposed local projection is:

   ```text
   GoalDisplayRow {
     key: (daemon, session_id), assignment_generation: Option<u64>,
     kind: Goal | Errand,
     objective, employee_name, job_title, project_label,
     execution: Pending | Running | Finished,
     status_label, status_icon, status_tone, reason?,
     created_at?, finished_at?, queue_position?,
     task_destination: Known | Resolving | Unavailable,
     wave: Option<{ id, display_name }>
   }
   GoalsListItem = SectionHeader | GoalRow | ErrandHeader | ErrandRow
   ```

   These are proposed display types, not existing protocol symbols. The row key
   stays stable on dispatch; generation disambiguates revival and stale results.
   Keep one current assignment per employee as today; do not create a new
   historical assignment store in this change.

2. **Close the objective/admission data gaps explicitly.** Prefer a current,
   generation-linked `ThreadGoal.objective`; otherwise use a persisted short
   assignment objective. Current employee sessions are explicitly titled with
   the employee name in `summon_employee` (`daemon.rs:7946–8045`), so
   `session.display_title()` is not a reliable assignment headline. Ticket.prompt
   is cleared after dispatch (`7595–7726`); reading it only while queued makes
   the headline disappear at launch.

   Add an optional persisted display objective at summon/requeue, derived
   deterministically from the original assignment's first nonempty paragraph,
   never from the persona wrapper. Keep the full assignment in the task; clamp
   only its list presentation. Legacy fallback is job title, then employee name,
   with no guessed objective. The preparation path must never hydrate every
   transcript or call a model to fill titles. Expose a generation-linked terminal
   outcome when available; until then use neutral Finished. Carry these fields
   through existing protocol/client generation and caller filtering.

   Add the queued assignment preview to the employee task surface from the same
   cached ticket, without fabricating a provider turn or user transcript message.
   Replace it with the dispatched transcript once the assignment is adopted.

   `SummonAdmission.queue_position` is returned on control/summon results, not
   stored on BossEmployee. Use a refreshed daemon display projection consistent
   with `BossService::queue_position` (`boss.rs:1070–1095`), which is per queue
   root. Do not number visible Goal rows: hidden errands/nested assignments also
   occupy places. Keep blockers from the persisted ticket. Future wave names
   and an explicit `executionState` projection can enrich this same boundary.

3. **Flatten section and row items outside render.** Compute goal/errand counts,
   attention counts, stable ordering, and disclosure items on relevant updates.
   Cache disclosure choices, scroll state, and focused row key per Boss daemon.
   Replace the count-only list reset with an item/order revision, including
   changes that preserve item count. Invalidate height measurement on width or
   content changes. Preserve the visible row anchor on state transitions and
   restore focus by key; if removed, focus the next row or its section header.

4. **Render the new surface in `src/app/right_panel.rs`.** Keep the current tab
   and `Goals` surface reuse. Use `div`, `list`, `ListState`, `scrollbar::vertical`,
   `px`/`sp`, `icon`, and the existing Theme tokens. Render rows from their
   snapshot with `&mut App` and `ActivationExt::on_activation_app`, deferring
   Waku updates to activation callbacks. Do not copy the current list builder's
   render-time `entity.update` lease; [performance](performance.md) prohibits
   reacquiring Waku when its caller already holds it.

5. **Wire keyboard and text handling.** Give headers/rows stable ids,
   `track_focus`, `tab_index`, `tab_group`/`tab_stop`, `focus_visible`, and
   `on_activation`/`on_activation_app` from [src/ui/mod.rs](../../src/ui/mod.rs)
   (`280–327`). Use one roving tab stop in the virtualized list: Up/Down and
   Home/End move among visible items, Enter/Space open tasks or toggle sections;
   Left folds a section and Right opens it. Focused items scroll into view.
   Capture callbacks without acquiring the entity until activation. Keep
   selection/focus distinct from completion tint. Localize new copy with `tr!`.
   Apply [UI conventions](ui-conventions.md): `#[track_caller]` for element
   constructors and matching child corner radii where child paint reaches edges.

## Follow-up verification

This proposal requires no Rust rebuild or visual test. For its implementation,
follow [testing](testing.md) and the existing dev watcher workflow in
[AGENTS.md](../../AGENTS.md). Use `mbx check` for compilation and focused tests
at the projection boundary for the consequential lifecycle contracts, rather
than screenshot snapshots or assertions about style constants.

Verify these behaviors in the rebuilt app with the actual provider/admission
interaction; visual analysis remains subject to the user's task authorization:

- A queued goal without a thread opens its assignment, shows its real wait
  reason/position, survives restart, and becomes Starting/Working once under
  capacity. An errand ahead of it still counts in its queue position.
- Revival with an old Complete thread goal shows the new pending assignment;
  dispatch keeps its headline and produces one row.
- Paused, Needs input, Usage limit, Budget reached, Failed, and neutral Finished
  stay distinct; a finished blocker remains discoverable when folded.
- Keyboard-only list/disclosure/task navigation has visible focus; returning
  from a task restores the Boss panel's scroll and disclosure choices. Check a
  remote queued task and a task whose transcript has not been loaded locally.
- At 280px and a wider panel, long objectives/names/reasons wrap or truncate
  according to the anatomy; full text remains available through the task.
  Resize, reorder with equal item count, and scroll a long finished history.
  Confirm row building remains proportional to visible items with no render I/O.
- No goals, errands only, finished goals only, loading, and offline snapshots
  each show the specified surface; both themes and reduce-motion remain legible.

The implementation needs executable `Test-Plan:` commit trailers. Follow
[changelog rules](changelog.md): update the existing unreleased
[Goals fragment](../../.changelog/feat-boss-goals-panel.md) for this refinement
rather than adding a second polish fragment. This proposal changes no shipped
behavior.
