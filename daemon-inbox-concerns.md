# Daemon Inbox Concerns

This file is intentionally more opinionated than the design doc. These are the parts of the design I think are still suboptimal, even if the overall direction is reasonable.

## 1. "Truly generic" is only partially true

The inbox contract is generic because it points at tagged daemon entity ids. The actual implementation is still only generic in theory.

In practice, v1 only works for:

- `ses_*` session entities
- `pr_*` pull request entities

Every new entity kind will still need:

- its own Kindstore kind
- its own read surface
- its own daemon-owned write path
- its own inbox-emission rules

That is probably the right level of pragmatism for now, but it means the system is "generic by contract, per-entity by implementation."

## 2. Reusing `pullRequests` as a first-class inbox entity is simple, but thin

This design intentionally avoids a new generic `artifacts` kind. I think that is the right simplification for v1.

The downside is that `pullRequests` is still a narrow daemon-local record, not a rich domain object. Right now it mostly exists so the daemon can remember:

- host
- owner
- repo
- PR number
- cwd

That is enough to key inbox items, but it is not much of an entity. If the product later wants richer daemon-owned outputs with common behavior, the repo may still end up introducing a real artifact layer anyway.

## 3. One row per entity means attention history is gone by design

This is the user's chosen model, and I agree it is simpler.

It is still worth stating plainly: once a row flips from `session.blocked` to `session.turn_ended`, or from `pull_request.created` to `pull_request.updated`, the older attention reason is gone from the inbox itself.

Per-turn `scope` and `headline` metadata gives us some record of what the user would have seen for past session turns. It does not create an audit trail of attention events, status changes, priority changes, or read/archive transitions.

That is fine if the inbox is only "what needs me now." It is a bad fit if people later start expecting notification history.

## 4. The per-turn suppression state is in-memory only

I think this is the right v1 tradeoff, but it is a real compromise.

The daemon needs some way to know whether to suppress `session.turn_ended` after a PR was already touched in the same turn. The lightest solution is an in-memory marker keyed by session id.

That means:

- daemon restart loses that marker
- the behavior is only correct while the live daemon still owns the turn

I think that is acceptable because daemon restart already breaks live session continuity. Still, it is not durable.

## 5. The "one inbox row per entity" rule still needs explicit conflict handling

Kindstore supports unique constraints on indexed columns, so the inbox should declare `entityId` as unique. That removes the old concern that duplicate rows are only prevented by single-daemon discipline.

The remaining risk is implementation-level: the inbox manager still needs to treat create-or-refresh as an atomic operation and handle unique-conflict races deterministically.

Cases to cover:

- two attention events touch the same entity close together
- one path tries to create while another path has just created
- a conflict occurs after some related session or pull-request state has changed

The storage shape can protect the invariant, but the manager still has to define which event wins and how `reason`, `status`, `priority`, `scope`, `headline`, and `updatedAt` are resolved.

My recommendation is to make the rules boring and explicit:

- all daemon attention goes through one `touchInboxItem(...)` helper
- unique-create conflicts refetch the row and apply the same update path
- daemon attention always reopens the row to `unread`
- daemon attention preserves priority unless it provides an explicit override
- latest daemon attention wins for `reason`, preview metadata, `turnId`, and `updatedAt`
- user updates win only when they happen later in store order
- bulk updates dedupe ids and apply one shared timestamp

This is not complicated, but it needs direct tests. Otherwise the code will look correct until two event paths race.

## 6. One `status` field is carrying three different concepts

The new `replied` status is the right distinction from `completed`. A reply means "no attention needed right now"; completion means "this entity is no longer a concern."

The remaining awkwardness is that `status` now mixes:

- attention state: `unread`, `read`, `replied`
- filing state: `saved`, `archived`
- lifecycle state: `completed`

Because this is a single field, the model cannot represent combinations like "saved and replied" or "archived but completed." That may be fine for v1, but the implementation needs clear precedence rules.

My recommended v1 rule is:

- daemon attention wins and sets `unread`
- implicit user replies move any existing non-archived session row to `replied`
- implicit user replies intentionally overwrite `saved` because the work is relevant again
- implicit user replies intentionally overwrite `completed` because the work is not done
- implicit user replies do not overwrite `archived`
- explicit user filing actions can set `saved` or `archived`
- `completed` is only set through entity-aware lifecycle paths

If product later needs independent filing and attention state, `saved`/`archived` probably need to move out of `status`.

## 7. Local-only inbox state will diverge across machines

This is an explicit product choice, not an accident.

It is also the biggest UX compromise in the design:

- mark an item read, replied, saved, completed, archived, or low-priority on one machine, and another machine does not see that state
- create attention on a laptop, desktop daemon does not know until it independently produces the same local row

If the product later wants "my inbox" rather than "this daemon's inbox," this design will not stretch very far.

## 8. The design still leaves some daemon authority split awkwardly

Moving blocker and turn-end reporting behind daemon IPC is clearly correct.

The awkward part is that PR submission and reply still live in the IPC server, while one-shot end-of-turn behavior lives in `SessionManager`, and inbox behavior spans both.

That is workable, but it means attention logic is not perfectly centralized. If the daemon grows more attention-producing flows, a stronger internal boundary may be needed.

## 9. Scope/headline quality depends on fallback behavior

The new session metadata model is useful only if the resulting rows stay concrete and subject-first.

The risky part is fallback synthesis. If fallback leans on generic session titles, command names, or vague agent output, the inbox will fill with rows that technically have a `scope` and `headline` but do not help the user decide what to open.

The implementation needs tests around concrete blocker, PR, and ordinary turn-ended cases, not just shape validation.

## 10. Terminal command ergonomics can affect compliance

Requiring a headline on every terminal flow is the right product rule, but it raises the burden on agent-facing commands.

The risk is that agents may either skip the command, produce repetitive filler, or stuff the chat response into the headline if the CLI shape is too awkward. The structured command should be easy to call from all terminal paths, especially `end-turn`, `report-blocker`, `submit-pr`, and `reply-pr`.

## Bottom line

I think the design is good enough to implement.

The parts I would watch most closely are:

1. whether reusing `pr_*` rows stays clean once there is a second non-session entity kind
2. whether in-memory turn suppression stays understandable once more daemon flows can touch entities
3. whether scope/headline fallback produces useful rows instead of plausible filler
4. whether the single `status` field starts fighting the difference between attention, filing, and lifecycle state
5. whether local-only inbox state becomes annoying faster than expected
