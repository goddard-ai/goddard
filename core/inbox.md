# Human Attention Inbox

## Goal

Provide a daemon-local inbox that helps humans decide which daemon-managed work needs attention now.

## Hypothesis

We believe that consolidating current attention state for sessions and pull requests will reduce missed handoffs, make agent work easier to scan, and give non-app hosts a shared control surface for attention workflows.

## Actors

- Human operator reviewing daemon-managed work.
- Agent producing session turns, blockers, pull requests, or pull request replies.
- Daemon acting as the local authority for inbox state.
- SDK, desktop app, and approved operational clients listing or updating inbox state.
- External reviewer whose managed pull request feedback may cause daemon activity.

## Core Model

- The inbox stores current attention state, not notification history.
- Each inbox row belongs to exactly one daemon-owned entity.
- Supported entities are daemon sessions and daemon-managed pull requests.
- Inbox rows identify entities through Goddard-generated internal identifiers, not external service identifiers such as GitHub pull request numbers.
- The daemon owns inbox row creation and attention refreshes.
- Clients may list inbox rows and update user workflow state, but they must not create rows or infer attention state independently.
- Inbox state is local to one daemon store. It is not a backend-synced cross-device inbox.

## Attention Events

The daemon should surface attention when:

- a session reports that it is blocked;
- a session turn ends without another entity taking responsibility for the turn's attention;
- a daemon-managed pull request is created;
- a daemon-managed pull request is updated by daemon-owned activity.

Daemon attention refreshes reopen the row as needing attention. A refresh should preserve the user's priority choice unless the daemon has a stronger explicit reason to change it.

## Inbox Status

Inbox status is a compact workflow state for the current row:

- `unread`: daemon-owned activity says the entity needs human attention.
- `read`: the user has visited or acknowledged the entity, and no newer daemon attention has arrived.
- `replied`: the user has replied to a related session, so no attention is needed until new daemon activity occurs.
- `saved`: the user intentionally parked the row for later.
- `archived`: the user hid the row from the active workflow.
- `completed`: the entity is no longer a current concern according to that entity's own lifecycle.

New rows default to unread attention with normal priority.

`completed` is not a generic synonym for handled. Completion is entity-specific:

- A session is completed only when the user explicitly completes the session's inbox concern.
- A pull request is completed only when the pull request is no longer open, such as after merge or closure.

Session replies are distinct from completion. When a user replies to a session that already has an inbox row, the row should become `replied` unless it is archived. This may overwrite `saved` or `completed` because the reply means the session is relevant again and is not done.

Archived rows remain archived when the user replies to the related session. New daemon attention may still reopen an archived row when the entity again requires human attention.

## Priority

Inbox priority is intentionally coarse:

- `normal`: the row participates in the standard attention queue.
- `low`: the row is still relevant but should sort behind normal-priority work.

Priority is a user workflow signal, not a replacement for status. Daemon attention should not casually erase priority choices.

## Session Inbox Metadata

Each terminal agent turn should provide hidden inbox metadata for the related session work:

- **Scope**: a short, semi-stable label for what the current work is about.
- **Headline**: a short, turn-specific update describing what changed or why the turn matters.

The preferred rendering is:

`{scope} — {headline}`

Scope should remain stable across related turns and change only when the work's center of gravity changes. Headline should be required for every terminal turn and should front-load the most important update.

This metadata is not part of the visible chat response. It exists so inbox rows can be scanned without forcing the agent to write a second user-facing message.

The daemon should preserve the latest usable scope for a session and reuse it when later turns omit scope. If metadata is missing or weak, the daemon should prefer a usable fallback over dropping the inbox update.

## User Workflow

- Visiting or acknowledging an entity may mark its inbox row read.
- Bulk user actions should behave as one user workflow action for ordering and display purposes.
- User workflow updates should not create inbox rows for entities that have never produced daemon attention.
- Later daemon attention should reopen rows that were previously read, replied, saved, archived, or completed.

## Non-Goals

- Aggregating external notification systems such as GitHub's own notification inbox.
- Providing append-only notification history or an audit trail of all attention events.
- Using external service identifiers as inbox links.
- Supporting complex custom routing, rule-based filtering, or per-user notification policies.
- Defining app visual design, truncation rules, or navigation layout.
- Syncing inbox state across multiple local daemon stores.

## Decision Memory

The inbox is intentionally one current row per entity because the immediate product need is "what needs attention now," not historical notification review.

`replied` and `completed` are separate states because replying means the user has handed control back to the agent, while completion means the entity is no longer a concern.

The inbox remains daemon-local for this phase to avoid introducing backend account-level state before the product proves that local attention triage is useful.
