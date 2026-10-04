# Goals panel design

Proposal only. Source reviewed on 2026-10-04 at `462b23cc`; revised after Alec's
review of `0fda8947`. No runtime or UI changes accompany this document. The
Boss chat's Goals tab should lead with **what finished**, then show ongoing and
pending goals and the task to open for each.

Use **Finished**, **In progress**, then **Pending**. Show about five recent
finished goals with a **Show more** expander for older history. Use compact
two-line rows: status icon and task title, then employee avatar/name, project
folder icon/name, worktree indicator, and relative last-updated time. Only
`EmployeeGoal::Goal` records belong here; exclude errands from the surface.
Keep projects as row metadata. Add named wave groups only when their product
contract exists. This keeps goals readable at the panel's 280px minimum width without
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
  A Goal row can exist without a thread goal. Filter by this work kind before
  preparing visible rows or counts; a provider thread goal does not make an
  Errand eligible for this tab. Work-kind terminology stays out of the UI.
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

At roughly 340px, recent finishes lead the panel. The sketch shows three of the
five default recent rows to keep it short. `[A]` denotes an employee avatar,
`[F]` the folder icon, and `[W]` the existing worktree fork icon; these labels
are diagram notation, not text rendered in the UI.

```text
┌────────────────────────────────────┐
│ Goals                         +    │  Existing tab strip
├────────────────────────────────────┤
│ Finished                         8 │  First; recent finishes expanded
│ ✓ Fix queued employee dispatch     │  Status icon + task title
│ [A] Nina [F] Goddard [W]     4m ago │  Identity, project, workspace, update
│ ✓ Check upgrade path               │
│ [A] Rowan [F] Goddard        8m ago │
│ × Validate release signing         │
│ [A] Leo [F] Goddard [W]     12m ago │
│   Signing identity unavailable     │  Optional small reason
│ … two more recent rows …           │  Annotation: up to five by default
│ Show more (3 older)                │  Bounded history expansion
├────────────────────────────────────┤
│ In progress                      2 │
│ ! Validate the new release flow    │
│ [A] Rowan [F] Goddard [W]    1m ago │
│   Needs input · Confirm identity   │  Attention reason remains explicit
│ ◌ Audit task navigation            │
│ [A] Nina [F] Goddard         2m ago │
│ Pending                          1 │
│ ◷ Audit keyboard navigation        │
│ [A] Leo [F] Goddard          3m ago │
└────────────────────────────────────┘
```

Normal rows have no persistent card background. A hover/focus background
reveals the rounded hit region, as in
employee sidebar rows and Automations. This makes spacing and type carry the
hierarchy rather than a stack of equally prominent cards.

Section headers carry counts; omit a second summary bar or large duplicate
Goals heading. Finished always comes first and starts expanded with the five
most recent results. `Show more (N older)` reveals the remaining finished rows;
`Show less` returns to the recent five. Echo the app's
[sidebar Show more row](../../src/app/sidebar.rs) (`4895–4942`) rather than
introducing a large button or paging controls.

When unfinished goals exist, cap the finished list viewport at the smaller of
five normal rows (about 240px) and 45% of available panel-body height. On short
panels, fewer recent rows are visible at once and scroll within that viewport.
Show more expands the available history **inside this same height cap**, so
older history never pushes In progress/Pending farther down. Reserve the rest
for the ongoing viewport; its sections scroll together. With finished goals
only, history may use the full body height. This is an intentional pair of
scroll regions, with the expansion control outside the finished viewport and
scrollbars only where needed.

In progress and Pending start expanded. Honor subsequent user folds and history
expansion choices per Boss daemon; updates do not force sections open. Suppress
empty sections. Every disclosure has an always-visible chevron and count.
Folded sections and the Finished header show `1 needs attention` when relevant,
including attention items older than the recent five, so they remain discoverable.

### Row anatomy and density

| Element | Proposed treatment | Existing surface to echo |
| --- | --- | --- |
| Panel body | 12px horizontal inset; bounded finished viewport above the ongoing viewport; no extra card frame | Current [Goals panel](../../src/app/right_panel.rs), `8253–8306`, supplies virtualized lists and overlay scrollbars |
| Section header | 28px target; 12px medium text; count tertiary; 12px gap before the next section | [Sidebar group header](../../src/app/sidebar.rs), `4620–4730`: quiet label, disclosure, hover and focus |
| Line 1: status + title | 12px status icon; 12.5px medium task title, `theme.text`, 16px line height, one line with ellipsis; assignment objective is the fallback | [Session status indicator](../../src/app/sidebar.rs), `5769–5833`, and [employee/sidebar labels](../../src/app/boss.rs), `1588–1652`, `2964–3013` |
| Line 2: employee | Cached 16px avatar, then 11.5px employee name in secondary text | [boss_avatar](../../src/app/boss.rs), `1301–1342`, and employee rows: reuse the identity and initial-letter loading fallback |
| Line 2: project/worktree | 11px `icons/folder.svg` then project name; add `icons/fork.svg` for an actual employee worktree | [Sidebar project chip](../../src/app/sidebar.rs), `24–41`, and worktree marker, `6325–6327` |
| Line 2: updated time | Trailing 11px tertiary relative last-updated label, kept visible | [format_time_ago](../../src/app/sidebar.rs), `1036–1043`; reuse its localized format with the update timestamp |
| Attention reason | Optional third line, 11.5px, single-line ellipsis; explicit state plus a short reason; full text in task/tooltip | [Automation run row](../../src/app/automations.rs), `1280–1387`: concrete failure/precheck detail and task destination |
| Hit region | Full row, 8px radius, 8px horizontal/vertical padding; `overlay` on hover, `focus_highlight()` on focus | [Automation row](../../src/app/automations.rs), `1147–1277` |

A normal row is about 48px; an attention reason adds 16px. Titles do not wrap
into another headline row. At 280px, line 2 remains a single line: preserve the
avatar, folder/worktree glyphs, and timestamp; truncate project and employee
names within flexible slots, project first. The full project path, workspace,
employee identity, title, status, and exact update time remain in the tooltip
and task surface. Do not replace the folder icon with the worktree marker:
project and workspace are separate facts. A queued worktree request has no
materialized worktree yet, so it gets no fork icon until that exists.

Keep the same two-line anatomy at wider widths. Do not show job title or a
separate status/budget line in every row. Status text must also be available
through keyboard focus and the task surface; attention states carry a small
reason line such as `Paused`, `Needs input · Confirm identity`, or `Budget
reached`. Pending admission details and queue position belong in focus-accessible
details, preserving the compact base row. Keep all text and icons theme-aware.

### Projects and waves

**Completion first, project on each row.** This panel answers “what finished?”
across the current Boss daemon, then shows unfinished work. Project-first nesting
would scatter finished, pending, and attention items and consume vertical space
with repeated headers. Always show
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
Finished                            8
  ✓ Verify signing …
  ✓ Check upgrade path …
  … up to five recent results …
  Show more (3 older)
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

Pending details stay neutral: `Queued · #1 · Waiting for swe-2 · 6/6 slots`, or
a sanitized host-resource explanation. These details appear on keyboard focus,
in the full-text tooltip, and in the queued task preview, not as another routine
row line. A later FIFO ticket with no recorded blocker says `Waiting for earlier
work`; #1 without a known reason says `Waiting for admission`. Never infer an
ETA, progress percentage, time spent executing, or stalled state from queue age.
Use `Queued` without a number when authoritative position is unavailable. An
execution attention/blocker state may keep its small explicit reason line.

For glyphs reuse loader-circle, hourglass, alert, x-bold, pause, and an existing
check glyph. Complete may use `theme.success`; budget/input/blockage use
`theme.warning`; queue/paused/neutral finish use secondary text. Pair every icon
with status text in focus-accessible details and the task surface; attention
states also show their reason inline. Animate only actual Starting/Working
indicators through the existing motion helper; waiting for capacity needs no spinner. Follow
[accessibility](accessibility.md) and reduce-motion settings.

## Navigation

The **whole goal row opens its employee task** using
`request_session_activation(session_id, SessionActivationTransition::Visit, cx)`,
as [employee sidebar rows](../../src/app/boss.rs) do (`1588–1652`). Employee text
names that destination; it is not a second link to the same place. Give the row
an `Open task: <title>, <employee>, <status>` label and tooltip containing
the full title/objective, job title, project, workspace, update time, and reason.
Expose those same details on keyboard focus. Hover is supplementary:
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
```

This message applies only to an authoritative snapshot containing no Goal
records, including when the Boss has only Errand employees. No work-kind labels
or excluded-work counts appear. When only finished goals remain, show their
recent finished section and Show more rather than an empty message. When the
Boss snapshot has not arrived, show `Loading goals…`; when unavailable, show `Goals unavailable`
with the existing host connection explanation. Retain a previous snapshot when
available and label it `Offline · last known state`; disconnection must not turn
running rows into finished rows or an empty list. Do not add per-row reconnect
buttons; use the app's existing host connection affordance.

## Implementation outline

1. **Prepare display data at snapshot boundaries.** Extend `BossGoalRow` and
   preparation in `drain_boss_events` (`src/app/boss.rs`). Filter employees to
   `work_goal == EmployeeGoal::Goal` before building panel rows and counts;
   leave the full roster intact for other surfaces and queue accounting. Refresh
   when relevant session/project/goal snapshots change as well as BossState. Build a session-id
   lookup once per refresh. Preserve `DaemonKey` ownership and stale-revision
   protection. Render only cached, immutable data; see [performance](performance.md).

   The proposed local projection is:

   ```text
   GoalDisplayRow {
     key: (daemon, session_id), assignment_generation: Option<u64>,
     task_title?, assignment_objective?, display_title,
     employee_name, employee_avatar_seed, cached_avatar?, job_title,
     project_label, workspace: Local | Worktree | Unknown,
     execution: Pending | Running | Finished,
     status_label, status_icon, status_tone, reason?,
     created_at?, finished_at?, updated_at?, relative_updated_label?, queue_position?,
     task_destination: Known | Resolving | Unavailable,
     wave: Option<{ id, display_name }>
   }
   GoalsListItem = SectionHeader | GoalRow
   FinishedHistory = { recent_limit: 5, show_older: bool, older_count: usize }
   ```

   These are proposed display types, not existing protocol symbols. The row key
   stays stable on dispatch; generation disambiguates revival and stale results.
   Keep one current assignment per employee as today; do not create a new
   historical assignment store in this change.

2. **Resolve task title, then assignment fallback.** Line 1 prefers the task's
   meaningful explicit/automatic title. When none exists, use the current,
   generation-linked `ThreadGoal.objective`, then a persisted assignment
   objective. Current employee sessions are explicitly titled with
   the employee name in `summon_employee` (`daemon.rs:7946–8045`), so
   `session.display_title()` may return an identity label rather than a task
   title. Track title provenance or expose a distinct task title; the seeded
   employee identity must not suppress the assignment-objective fallback. Do
   not replace a meaningful task title with its objective. Ticket.prompt
   is cleared after dispatch (`7595–7726`); reading it only while queued makes
   the headline disappear at launch.

   Add an optional persisted display objective at summon/requeue, derived
   deterministically from the original assignment's first nonempty paragraph,
   never from the persona wrapper. Keep the full assignment in the task; clamp
   only its list presentation. Legacy fallback is job title, then `Untitled goal`,
   with no guessed objective; the employee name already appears on line 2.
   The preparation path must never hydrate every transcript or call a model to
   fill titles. Expose a generation-linked terminal
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

3. **Prepare identity, workspace, and update metadata.** Reuse the existing
   Boss avatar cache/pump for 16px rasters; prepare a cached image or initial-letter
   fallback without reacquiring Waku from the list row builder. Use the actual
   cached session workspace for the fork marker, not the requested worktree
   mode on a still-queued ticket. Preserve folder and worktree as separate icons.
   Derive the relative timestamp from the current task's `updated_at`, including
   a newer known admission/finish timestamp if the session snapshot lags.
   It means last updated, not last reply or time spent executing. If no update
   timestamp is known, omit it. Reuse `format_time_ago` with saturating elapsed
   subtraction; refresh labels at minute boundaries through the existing clock
   cadence, without changing row sort order or starting a per-row timer.

4. **Flatten section and row items outside render.** Order Finished, In progress,
   Pending. Compute goal-only counts, attention counts, and stable ordering on
   relevant updates. Initially prepare five recent finished rows; Show more
   admits older rows to the same bounded history viewport and Show less restores
   the five-row limit. Keep its height cap when ongoing work exists, including
   when attention reasons make rows taller. Do not let an expanded history
   enlarge the whole panel's scroll content above the ongoing sections.
   Cache disclosure/history expansion choices, both list/scrollbar states, and
   focused row keys per Boss daemon.
   Replace the count-only list reset with an item/order revision, including
   changes that preserve item count. Invalidate height measurement on width or
   content changes. Preserve the visible row anchor on state transitions and
   restore focus by key; if removed, focus the next row or its section header.

5. **Render the new surface in `src/app/right_panel.rs`.** Keep the current tab
   and `Goals` surface reuse. Use two `list`/`ListState` viewports with
   `scrollbar::vertical`, `div`, `px`/`sp`, `icon`, cached avatar images, and the
   existing Theme tokens. Put the Finished header and Show more/less control
   outside its scrolling rows. Render line 1 as status icon + one-line title;
   line 2 as avatar + employee + folder icon + project + optional fork + trailing
   relative update. Add a small reason line only for attention/blocker states.
   Render rows from their snapshot with `&mut App` and
   `ActivationExt::on_activation_app`, deferring
   Waku updates to activation callbacks. Do not copy the current list builder's
   render-time `entity.update` lease; [performance](performance.md) prohibits
   reacquiring Waku when its caller already holds it.

6. **Wire keyboard and text handling.** Give headers/rows and Show more/less
   stable ids,
   `track_focus`, `tab_index`, `tab_group`/`tab_stop`, `focus_visible`, and
   `on_activation`/`on_activation_app` from [src/ui/mod.rs](../../src/ui/mod.rs)
   (`280–327`). Use one roving tab stop per virtualized viewport: Up/Down and
   Home/End move among visible items, Enter/Space open tasks or toggle sections;
   Left folds a section and Right opens it. Tab traverses the history expander
   and ongoing viewport in section order. Focused items scroll into their own
   viewport; opening older history must not steal focus or scroll the ongoing
   viewport. If Show less hides the focused row, return focus to that control.
   Provide full row/status details on keyboard focus as well as pointer hover.
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
  dispatch keeps its fallback headline and produces one row. A meaningful task
  title takes precedence over its assignment objective; an identity-only title
  does not.
- Paused, Needs input, Usage limit, Budget reached, Failed, and neutral Finished
  stay distinct; an older finished blocker remains discoverable through the
  header attention count and Show more.
- Keyboard-only list/disclosure/task navigation has visible focus; returning
  from a task restores the Boss panel's scroll and disclosure choices. Check a
  remote queued task and a task whose transcript has not been loaded locally.
- At 280px and a wider panel, rows remain two lines with ellipsized titles and
  metadata; only attention reasons add a small line. Avatars, folder/worktree
  icons, and relative updates stay visible. Full text remains available through
  focus details and the task. Resize, reorder with equal item count, and scroll
  a long finished history.
  Confirm row building remains proportional to visible items with no render I/O.
- Finished appears first, newest first, with five recent rows by default.
  Show more/less and a short panel keep history bounded and the ongoing viewport
  in place. With finished goals only, history may fill the body. Neither title
  changes nor timestamp ticks reorder rows.
- No goals, excluded-work-only, finished goals only, loading, and offline snapshots
  each show the specified surface; both themes and reduce-motion remain legible.
  Errand employees never create rows, headers, terminology, or counts here,
  including when they have a provider thread goal.

The implementation needs executable `Test-Plan:` commit trailers. Follow
[changelog rules](changelog.md): update the existing unreleased
[Goals fragment](../../.changelog/feat-boss-goals-panel.md) for this refinement
rather than adding a second polish fragment. This proposal changes no shipped
behavior.
