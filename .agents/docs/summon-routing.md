# Automatic routing for Boss summons

## Recommendation

Make `route: "auto"` the Boss's normal summon instruction. The daemon classifies
one bounded assignment as Easy, Medium, or Hard, resolves the user's existing
class preference to provider/model/effort, then submits that concrete target to
the employee admission queue. Explicit selection wins. Keep model names and
routing heuristics out of reusable persona instructions.

This is a proposed design, not implemented behavior. It covers initial employee
selection; autonomous model handoffs and the queue's cap policy remain separate.

## Current plumbing and evidence

Investigated 2026-10-04. **M** is the assigned `main` checkout at `fed5d308`;
it contains routing but no Boss service. **D** is committed development source
at `c5db8949490fc01e7e6734bd09b92c7058f91631`, read with `git show` because the
`dev` worktree has unrelated in-flight changes. References below use those
revisions, not the live daemon's unknown build revision.

| Area | Confirmed behavior | Evidence |
| --- | --- | --- |
| Preferences | `DaemonSettings.route_classes` maps Easy/Medium/Hard to provider, optional model and effort. `provider_route_classes` bounds same-provider mid-session moves; it is not the initial cross-provider map. | M `crates/waku-protocol/src/settings.rs:65`, `crates/waku-protocol/src/routing.rs:101` |
| Settings UI | Jev settings renders three class cards; selecting a model saves its effort in the target. | M `src/app/settings.rs:8597`, `src/app/settings.rs:9387` |
| Classification | `route_task` asks one `class` Choice, defaults ordinary engineering to Medium, and requires confidence ≥0.55 when supplied. The current code accepts absent confidence as 1.0. | M `crates/waku-core/src/routing.rs:30`, `:63`, `:157` |
| Resolution and degradation | Class-map resolution checks provider/model eligibility. Missing backend, failed/missing answer, low confidence or unmapped class uses eligible `last_used`, then the first candidate's provider default. An unlisted mapped model uses its provider default. | M `crates/waku-core/src/routing.rs:94`, `:204`, `:248` |
| Summon | `BossOperation::Summon` has optional provider/model, no route or effort field. It passes selection to shared task creation, holding `operation_lock` through creation/launch. A successful summon adds the live employee-card marker to its supervisor's transcript. | D `crates/waku-protocol/src/boss.rs:193`, `crates/waku-core/src/daemon.rs:6122` |
| Existing Auto shortcut | Summon's shared creation path already recognizes `model: "auto"` with no provider. It calls `route_agent_task` on the assignment text; otherwise omitted selection inherits the supervisor. Candidates are installed/enabled providers with cached/fallback catalogs; fallback is the most recently updated non-incognito eligible session. | D `crates/waku-core/src/daemon.rs:5512`, `:7287` |
| Session provenance | Task creation persists `route_decision`. `auto_route` is a draft flag, meaningless after start; ownership/provenance is the decision. `setModel` clears both. | D `crates/waku-core/src/daemon.rs:5624`, `:6257`; `crates/waku-protocol/src/model.rs:1655` |
| Evaluation and effort | `evaluate_with_feature` is shared by `Command::Evaluate` and embedded daemon decisions; the context router snapshots state and evaluates off-thread. The desktop turn router separately evaluates actual model effort options with a 0.70 confidence threshold. | D `crates/waku-core/src/daemon.rs:51`, `:5920`; M `src/app/routing.rs:217` |

The assignment reports persona-driven literal model choices. The inspected
summon path confirms that such explicit choices are honored; it does not establish
which routing rules exist in persisted user personas. No granted memory was read.
See [Jev conventions](jev.md) for backend, logging and spend rules.

## Protocol sketch

Extend the existing flat `Summon` payload with optional `route` (`auto` or
`explicit`) and optional `reasoningEffort`. Keep provider/model fields for
compatibility. These are proposed wire names:

```json
{
  "type": "summon",
  "personaId": "<existing-persona-uuid>",
  "jobTitle": "Summon Routing Design",
  "prompt": "Write a bounded design for automatic employee routing.",
  "project": "/Users/alec/dev/goddard-ai/goddard",
  "route": "auto"
}
```

| Request | Effective selection |
| --- | --- |
| `route: "auto"`, no literal provider/model | Classify and resolve global class map |
| Literal provider and/or model, even with `route: "auto"` | Explicit selection wins; skip classification and report `explicit-override` |
| `route: "explicit"`, omitted provider/model | Existing supervisor inheritance |
| Omitted route and selection | Preserve legacy inheritance during rollout |
| Legacy `model: "auto"`, no provider | Alias for automatic routing |

`model: "default"` explicitly selects the provider default. A lone literal model
uses the supervisor's provider; do not guess a harness from a model id. Preserve
the current rejection of provider plus the legacy `model: "auto"` sentinel.
New Boss instructions should send `route: "auto"` unless the human named a target.
Do not silently rewrite user-owned personas or stored routing constraints.

Extend `BossResult::Summoned` additively with `selection` (resolved target), optional
`routeDecision`, and queue-owned `status` (`routing`, `queued`, `starting`,
`running`, `failed`, `cancelled`). Pending selection may be absent. Return the
employee/session id immediately after durable acceptance; transitions arrive via
normal state updates. Existing callers may continue reading only `sessionId`.
Update generated client types, CLI schema and Rhai `summon` help together.

## Daemon ownership and resolution

Classification belongs in the summon pipeline, not a Boss-authored eval script.
A Boss eval would duplicate settings access, fallback and validation, and let
prompt instructions bypass the preference contract. Reuse the daemon evaluator
behind `Command::Evaluate` directly through `evaluate_with_feature`; do not issue
recursive IPC to the same daemon. Refactor shared routing into classification
and deterministic resolution seams rather than copy its rules.

At acceptance, validate authorization, persona grants, project/workspace inputs,
and reserve the employee id under the existing operation lock. Persist the
assignment before returning. Release that lock before provider probing, network
calls, queue waits or launches. A bounded worker snapshots settings and candidates,
evaluates once, then applies only if the request is still pending. Cancellation
and explicit `setModel` invalidate the pending decision; recheck grants and
availability before admission. Never retain task-state locks across evaluation.

Use structured, bounded state: `jobTitle` (256 characters), assignment `prompt`
(8,000), project label, and optional explicit job context (4,000). Job context is
a relevant task summary, not the entire Boss digest. Do not automatically fetch
memory, raw tool arguments or persona files. Treat titles and context as evidence,
not routing commands; Jev sees no model ids. Reuse `routing_questions` and its
confidence policy initially, with a five-second evaluation budget. Log under
`boss-summon-route`, linked to employee/supervisor ids, and add the settings label.
Log the applied target/reason as well as the judgment; avoid duplicate eval logs.

Recommended summon-specific failure default: the user's eligible **Medium**
preference, then a snapshot of the supervisor's eligible target, then the first
eligible candidate's provider default. This keeps ordinary delegated work on the
user's workhorse when Jev is unavailable and avoids unrelated sessions changing
the fallback during a batch. This differs from today's daemon-wide last-used
fallback; implement it as an explicit summon fallback policy, leaving normal task
routing unchanged. Reuse eligibility resolution and record each degradation in
`reason`. Never claim Jev chose Medium on failure: `class` is absent and the reason
is, for example, `eval-failed+fallback-medium`. An unavailable mapped model may
resolve to its provider default, visibly recorded. No launchable provider means
an actionable failed summon, not a retry loop.

## Effort and cap composition

Start with the class target's configured effort; add an explicit summon effort
override and validate it against the resolved catalog entry's `reasoning_efforts`.
An unsupported explicit effort fails clearly. An unsupported configured effort
degrades to the model default with a recorded reason. Unset effort uses the model
default; do not inherit an unrelated supervisor model's effort on an Auto summon.
Keep explicit/legacy inheritance behavior compatible.

Do not add a generic low/medium/high classifier in phase one: supported ladders
vary by model, and the class map already stores the user's effort preference.
If calibration later shows value, add opt-in effort evaluation after model
resolution, asking only that model's actual supported ids. Reuse the existing
turn effort threshold/default behavior; failures retain configured effort.
Do not enable per-turn handoffs as a side effect of this feature.

The queue contract is:

`Jev tier → user preference → concrete provider/model/effort → cap admission → launch`.

Resolve provider-default selections to a concrete catalog model before choosing
the queue's canonical model key. Routing jobs do not consume employee model slots;
limit evaluator concurrency separately. Queue the selected model when its cap is
full: never downshift tiers to evade caps. Freeze resolved selection while queued;
settings changes affect new requests. Revalidate availability and current caps
at admission, and surface failure if the target disappeared. The parallel queue
implementation owns fairness, cap-key normalization, restart recovery and slot
release. Automatic routing must not bypass those mechanisms; cap-controlled model
changes need the same admission path.

## Visibility and phased delivery

Show `Auto · Medium · <model> · <effort>` on the existing summon transcript card,
with textual queue state. Show `Auto fallback · <model>` for degraded resolution
and expose its reason in details. Include the same fields in `view`/`context` so
the Boss can inspect the choice without reading an employee transcript. Preserve
initial route provenance if a later explicit override changes the current target.
The sidebar already reveals model/effort with Option (D `src/app/boss.rs:1523`);
route explanation should also be available through keyboard-accessible details.

1. Add protocol fields and shared resolution seams. Preserve omitted-field
   behavior and the existing Auto alias. Verify explicit precedence, catalog
   effort validation, configured preferences, and each fallback with fake evals.
2. Integrate the durable async summon state with the parallel queue work. Verify
   classification once per request, cancellation during evaluation, stale model
   overrides, capped-model queuing, settings snapshots and restart recovery.
3. Update Boss instructions, schema/Rhai help and card/view projections. Calibrate
   bounded assignments under `boss-summon-route`; check tier distribution, fallback
   frequency, latency and user corrections before considering independent effort
   classification. Human verification should cover Auto, explicit and Jev-down
   summons plus queue admission in the rebuilt debug app.
