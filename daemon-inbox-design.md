# Daemon Inbox Implementation Plan

## Objective

Implement a daemon-local inbox that tracks current human-attention state for daemon-owned entities.

This plan keeps the agreed product shape:

- one inbox row per entity
- entity identity comes from tagged daemon ids
- inbox state is local to one daemon store
- item state includes `status` and `priority`
- session inbox rows include a scannable latest-turn preview made from `scope` and `headline`
- clients can update one item or bulk-update many items
- app behavior is deferred

## Fixed Decisions

These decisions are settled and should not be revisited during implementation:

- Inbox entities are `session` and `pull_request` in v1.
- The inbox stores current attention state, not append-only notification history.
- Inbox rows are keyed by `InboxEntityId`, not synthetic `type/id` links.
- New daemon-generated attention creates or refreshes rows as:
  - `status = "unread"`
  - `priority = "normal"` for new rows
  - existing `priority` preserved on later attention unless the daemon explicitly overrides it
- Inbox item statuses are `unread`, `read`, `replied`, `completed`, `saved`, and `archived`.
- Inbox item priorities are `normal` and `low`.
- Bulk updates apply one shared `updatedAt` timestamp to every affected row.
- `replied` means the related session has received a user reply and does not need attention until a later daemon attention event.
- `completed` is entity-specific and must not mean "has been replied to":
  - a completed session is one the user explicitly completes
  - a completed pull request is one the daemon knows has been merged or closed
- Session turn inbox metadata is hidden state, not a user-visible chat message.
- Session turn metadata has a sticky `scope` and a required `headline`.
- `scope` is stored as sticky session metadata and reused until the agent supplies a new non-empty value.
- `headline` is required at the reporting boundary, but the daemon still needs fallback synthesis for malformed or missing metadata.
- Session inbox rows store the latest resolved `scope`, latest `headline`, and latest `turnId` for row preview rendering.
- `goddard-tool` must stop mutating `db.sessions` directly for initiative, blocker, and turn-ended reporting.
- Pull request creation and reply remain daemon-owned flows. Clients do not create inbox entities directly.
- Turn history stays in `sessionTurns` and `sessionTurnDrafts`. Per-turn inbox metadata may live on completed turn records, while inbox item status and priority remain a separate read model.

## Status Semantics

The inbox status is a compact workflow state for the current row:

- `unread`: daemon-owned activity says the entity needs human attention.
- `read`: the user has seen the item, but the entity may still need a decision or follow-up.
- `replied`: the user has replied to the related session, so no attention is needed until the daemon produces new attention.
- `saved`: the user intentionally parked the item for later.
- `archived`: the user hid the item from the active inbox.
- `completed`: the entity is no longer a current concern according to that entity's lifecycle.

Completion is not a generic inbox filing action. It is applied through entity-aware paths:

- `ses_*`: a future app "Complete" action should call a daemon or SDK method that marks the session inbox row completed. This must not terminate or delete the session.
- `pr_*`: the daemon should mark the PR inbox row completed only when it has observed the PR as merged or closed. Replying to a PR, replying to a session about a PR, or viewing a PR must not mark it completed.

Daemon attention events are reopen events. They set the row back to `unread` even if the row was `read`, `replied`, `saved`, `archived`, or `completed`. A completed row receiving new daemon attention should be rare; if it happens, surfacing the new attention is safer than silently preserving completion.

User reply handling is separate from completion. When the daemon observes a user message sent to a session, it should mark that session's inbox row `replied` if the row is currently `unread` or `read`. It should not silently overwrite `saved`, `archived`, or `completed`; those states require an explicit user action or a new daemon attention event.

## Scope

In scope:

- `core/schema`
- `core/daemon`
- `core/sdk`
- daemon-facing CLI wiring in `core/daemon/src/bin/goddard-tool.ts`
- app-side daemon IPC contract parity only if compile or test surfaces require it

Out of scope:

- app inbox UI, routing, or automatic mark-read behavior
- backend-synced inbox state
- generic artifacts beyond existing session and pull request entities
- final UI truncation, sender identity, or visual treatment for scope/headline rows

## Workstreams

### 1. Schema Contracts

Package:

- `core/schema`

Files to add or update:

- `core/schema/src/common/params.ts`
- `core/schema/src/daemon/store.ts`
- `core/schema/src/daemon/inbox.ts` (new)
- `core/schema/src/daemon-ipc.ts`
- `core/schema/src/daemon.ts`

Tasks:

- Add `DaemonPullRequestId` and `DaemonPullRequestIdParams` so pull requests are addressable by tagged id in the same way sessions already are.
- Add inbox boundary types in a dedicated schema module:
  - `InboxEntityId`
  - `InboxReason`
  - `InboxStatus`
  - `InboxPriority`
  - `InboxItem`
  - `InboxScope`
  - `InboxHeadline`
  - `SessionInboxMetadataInput`
  - `ListInboxRequest`
  - `ListInboxResponse`
  - `UpdateInboxItemRequest`
  - `UpdateInboxItemResponse`
  - `BulkUpdateInboxItemsRequest`
  - `BulkUpdateInboxItemsResponse`
  - `CompleteSessionRequest`
  - `CompleteSessionResponse`
  - `GetPullRequestResponse`
- Add a durable store shape for inbox rows in `core/schema/src/daemon/store.ts`.
- Export the new schema types through `core/schema/src/daemon.ts`.
- Extend `daemonIpcSchema` with:
  - `prGet`
  - `inboxList`
  - `inboxUpdate`
  - `inboxBulkUpdate`
  - `sessionComplete`
  - `sessionDeclareInitiative`
  - `sessionReportBlocker`
  - `sessionReportTurnEnded`
- Add structured inbox metadata fields to terminal agent request payloads:
  - `scope?: InboxScope`
  - `headline: InboxHeadline`

Acceptance criteria:

- The new types compile and are exported from `@goddard-ai/schema/daemon`.
- `InboxEntityId` is the single alias used across new inbox request and response types.
- `InboxStatus` includes `replied`, and `completed` is documented as entity-lifecycle state rather than a generic "handled" state.
- The IPC contract fully describes single-item and bulk inbox mutation.
- The IPC contract exposes an entity-aware session completion path instead of relying on generic inbox update semantics for completion.
- Terminal session reporting requests carry structured inbox metadata, not freeform formatted display strings.

### 2. Kindstore Persistence

Package:

- `core/daemon`

Files to update:

- `core/daemon/src/persistence/store.ts`

Tasks:

- Add a new Kindstore kind `inboxItems` with tag `inb`.
- Use the shared inbox store schema from `@goddard-ai/schema/daemon/store`.
- Store inbox row status with default `unread` and priority with default `normal`.
- Add sticky session inbox scope to the stored session shape.
- Add resolved per-turn inbox metadata to completed session turn records:
  - `inboxScope`
  - `inboxHeadline`
- Add indexes for the actual query shapes:
  - unique `entityId`
  - `status`
  - `updatedAt_id`
- Keep `createdAt` out of the inbox kind.
- Store latest session preview fields on session inbox rows:
  - `scope`
  - `headline`
  - `turnId`
- If PR lifecycle completion is implemented in this phase, store enough authoritative PR lifecycle state on `pullRequests` to distinguish `open`, `closed`, and `merged`. Do not infer completion from PR comments or agent replies.

Acceptance criteria:

- The daemon store opens cleanly with the new kind declaration.
- The `entityId` index enforces one inbox row per entity.
- Inbox rows are queryable by entity id and status.
- Inbox list pagination can sort by `updatedAt desc, id desc`.
- The latest session inbox row preview can be rendered from stored `scope` and `headline` without reading turn history.
- Completed turns preserve the resolved scope/headline that was active for that turn.

### 3. Internal Inbox Ownership

Package:

- `core/daemon`

Files to add or update:

- `core/daemon/src/inbox/manager.ts` or equivalent new module
- `core/daemon/src/ipc/server.ts`
- `core/daemon/src/session/manager.ts`

Tasks:

- Add a dedicated daemon-owned inbox manager responsible for:
  - listing inbox items
  - updating one inbox item
  - bulk-updating inbox items
  - touching or refreshing one inbox row from daemon-owned events
  - marking a session row `replied` after a user reply when the current status is `unread` or `read`
  - marking a session row `completed` through an explicit session-completion path
  - updating latest session preview fields from resolved turn metadata
  - loading pull request entities by tagged id
- Keep inbox write semantics centralized in this module instead of duplicating them across IPC handlers.
- Implement all daemon attention writes through a single `touchInboxItem(...)` path:
  - compute one event timestamp
  - try to update the existing row by `entityId`
  - create the row when no existing row is found
  - if create hits the unique `entityId` constraint, refetch and update the existing row
  - set `status = "unread"`
  - set `priority = "normal"` only for new rows unless an explicit daemon priority override exists
  - preserve existing priority on later attention when no override exists
  - use latest daemon attention values for `reason`, preview metadata, `turnId`, and `updatedAt`
- Implement inbox row creation through the Kindstore unique `entityId` constraint so races cannot create duplicate rows.
- Implement entity-aware status validation:
  - `read`, `replied`, `saved`, and `archived` are ordinary inbox workflow states
  - `completed` must route through an entity-specific policy and must not be used to mean "the user replied"
  - session completion is allowed through the explicit session-completion path
  - pull-request completion is allowed only when the daemon has observed the PR as merged or closed
- Implement bulk update semantics exactly once:
  - dedupe requested ids
  - reject empty `entityIds`
  - reject requests with no mutable fields
  - compute one mutation timestamp
  - apply that same `updatedAt` to every affected row
  - return `missingEntityIds` without creating rows
  - apply the same entity-aware status validation as single-item updates

Acceptance criteria:

- Single-item and bulk-item updates share the same validation rules for mutable fields.
- Repeated or concurrent attention for the same entity cannot create duplicate inbox rows.
- Unique-conflict races retry by refetching the row and applying the same deterministic touch semantics.
- Later daemon attention wins for attention-owned fields and reopens the row to `unread`.
- User updates win only when they are ordered after daemon attention by the store.
- Bulk updates apply one shared timestamp across all affected rows.
- Missing ids are reported, not silently dropped and not auto-created.
- Session inbox row refreshes atomically update reason, status, latest scope, latest headline, latest turn id, and `updatedAt`.
- Session reply updates mark only `unread` or `read` session rows as `replied`.
- Completion is not accepted as a generic synonym for "handled" or "replied."

### 4. Session Lifecycle Integration

Package:

- `core/daemon`

Files to update:

- `core/daemon/src/session/manager.ts`
- `core/daemon/src/ipc/server.ts`
- `core/daemon/src/bin/goddard-tool.ts`

Tasks:

- Add daemon-owned session reporting methods:
  - `declareInitiative`
  - `reportBlocker`
  - `reportTurnEnded`
  - `completeSession`
- Move `goddard declare-initiative`, `goddard report-blocker`, and the terminal turn command to those IPC methods.
- Add a structured terminal CLI command:
  - `goddard end-turn --headline "..." --scope "..."`
  - `goddard end-turn --json '{"scope":"...","headline":"..."}'`
- Extend terminal CLI flows that can end a turn with the same structured metadata:
  - `report-blocker`
  - `submit-pr`
  - `reply-pr`
- Remove direct `db.sessions` mutation from `goddard-tool`.
- Keep per-session turn-entity activity as session-lifecycle state inside the daemon, not in the CLI.
- Reuse the same session-manager path for one-shot turn-ended handling so the daemon does not split that logic across unrelated codepaths.
- Wire user replies through the inbox manager:
  - when `sessionSend` accepts a user message, mark the session inbox row `replied` if it exists and is currently `unread` or `read`
  - do not mark a session `completed` because the user replied
  - do not overwrite `saved`, `archived`, or `completed` from the implicit reply path
- Implement session completion as an explicit entity-aware operation:
  - mark the session inbox row `completed`
  - update `updatedAt`
  - do not terminate, delete, or archive the session
- Resolve session inbox metadata at turn end:
  - validate the supplied headline
  - use supplied scope when present
  - otherwise reuse the sticky session scope
  - synthesize missing or malformed values through the fallback path
  - store the resolved scope/headline on the completed turn
  - update the sticky session scope when a valid new scope is supplied
  - update the session inbox row preview when the session row receives attention

Acceptance criteria:

- `goddard-tool` no longer imports `db` for session report commands.
- Blocker reporting creates session inbox attention.
- Turn-ended reporting creates session inbox attention only when no other attention-carrying entity was touched during that turn.
- `declare-initiative` does not clear inbox rows.
- Every terminal session report produces a resolved headline.
- Scope remains stable across turns when the agent omits it.
- A new valid scope updates the sticky session scope for later turns.
- User replies move active session inbox attention to `replied`, not `completed`.
- Explicit session completion is separate from reply handling and does not affect session runtime state.

### 5. Pull Request Integration

Package:

- `core/daemon`

Files to update:

- `core/daemon/src/ipc/server.ts`

Tasks:

- Add `prGet` to fetch one stored daemon pull request by tagged id.
- After `prSubmit` succeeds:
  - update session PR permissions
  - create or update the local `pullRequests` row
  - resolve and store session turn inbox metadata from the supplied PR terminal metadata or fallback data
  - touch the corresponding inbox row with `pull_request.created`
  - mark the current session turn as having touched another attention entity
- After `prReply` succeeds:
  - update the local `pullRequests` row
  - resolve and store session turn inbox metadata from the supplied PR terminal metadata or fallback data
  - touch the corresponding inbox row with `pull_request.updated`
  - mark the current session turn as having touched another attention entity
- Do not treat `prReply` as PR completion. A replied-to PR may still be open and still be a concern.
- Add PR completion handling only where the daemon has observed authoritative PR lifecycle state:
  - merged PRs set the PR inbox row to `completed`
  - closed PRs set the PR inbox row to `completed`
  - open PRs remain governed by normal attention events and user workflow status
  - if this implementation does not add a provider-state refresh path, leave automatic PR completion deferred rather than guessing from replies or comments

Acceptance criteria:

- The inbox uses the returned `pr_<...>` id from `recordPullRequest(...)`.
- Pull request activity resets the inbox row to `status = "unread"`.
- Existing priority is preserved unless explicitly overridden by daemon logic.
- PR terminal flows still preserve session turn scope/headline metadata even when session inbox attention is suppressed in favor of the PR row.
- PR reply does not mark the PR inbox row `completed`.
- PR completion is derived from observed merged or closed state, not from reply activity.

### 6. Metadata Validation And Fallback

Package:

- `core/daemon`

Files to add or update:

- `core/daemon/src/inbox/metadata.ts` or equivalent helper
- `core/daemon/src/session/manager.ts`
- `core/daemon/src/ipc/server.ts`

Tasks:

- Centralize metadata validation and normalization in one helper.
- Enforce basic input constraints:
  - headline must be non-empty after trimming
  - scope must be non-empty when supplied
  - scope and headline must stay within documented length limits
  - headline should reject or repair first-person openings
  - scope should reject or repair generic labels such as `task`, `update`, or `work in progress`
- Implement a fallback path that can synthesize usable metadata from available daemon context:
  - sticky session scope
  - session title or initiative
  - blocker reason
  - PR title or reply context
  - latest turn content when available
- Log quality events for:
  - missing headline
  - missing scope with no sticky scope
  - rejected or repaired headline
  - rejected or repaired scope
  - fallback metadata synthesis

Acceptance criteria:

- Weak or missing metadata does not prevent the daemon from producing a usable session inbox row.
- Fallback keeps scope stable when there is no clear focus shift.
- Validation and fallback behavior are testable without app UI.

### 7. SDK Surface

Package:

- `core/sdk`

Files to update:

- `core/sdk/src/sdk.ts`

Tasks:

- Add thin daemon-backed SDK methods:
  - `sdk.pr.get(...)`
  - `sdk.session.complete(...)`
  - `sdk.inbox.list(...)`
  - `sdk.inbox.update(...)`
  - `sdk.inbox.bulkUpdate(...)`
- Keep the SDK as a direct mirror of daemon IPC semantics.

Acceptance criteria:

- The SDK surface exposes all new daemon IPC methods needed by the inbox feature.
- Single-item and bulk-item update methods preserve the daemon request and response shapes without extra SDK-specific behavior.

### 8. App Contract Parity

Packages:

- `app`

Files to update if needed:

- any app-side daemon IPC consumer, type wrapper, or contract test touched by the daemon IPC additions

Tasks:

- Keep typed daemon IPC consumers compiling after the new methods land.
- Do not add app inbox behavior in this change.

Acceptance criteria:

- The app still compiles against the updated daemon IPC schema.
- No inbox UI work is included.

## Execution Order

Implement in this order:

1. Schema contracts in `core/schema`
2. Kindstore inbox kind in `core/daemon/src/persistence/store.ts`
3. Internal inbox manager
4. Metadata validation and fallback helper
5. Session lifecycle reporting methods and CLI rewiring
6. Pull request integration
7. IPC handlers
8. SDK methods
9. Test updates
10. Spec update as a merge prerequisite

This order keeps public types ahead of daemon code, and daemon code ahead of SDK wrappers.

## Verification Plan

### Schema tests

- Assert new inbox schema exports compile and serialize as expected.
- Assert `daemonIpcSchema` includes the new inbox and reporting requests.

### Daemon tests

- creating attention for a new entity creates one inbox row
- later daemon attention reuses the same row, resets `status` to `unread`, and preserves `priority`
- repeated attention for the same entity cannot create duplicate inbox rows
- create-vs-create races resolve to one row by refetching after unique `entityId` conflicts
- daemon attention after `read`, `replied`, `saved`, `archived`, or `completed` reopens the row to `unread`
- daemon attention preserves existing priority unless a daemon override is explicitly provided
- inbox list defaults to unread-only behavior
- inbox list pagination orders by `updatedAt desc, id desc`
- `inboxUpdate` is idempotent
- `inboxBulkUpdate` is idempotent for repeated identical requests
- `inboxBulkUpdate` applies one shared `updatedAt` to all affected rows
- `inboxBulkUpdate` reports missing ids without creating rows
- `inboxBulkUpdate` uses the same entity-aware status validation as single-item updates
- new inbox rows default to `priority = "normal"`
- user reply to a session marks an existing `unread` or `read` session inbox row as `replied`
- user reply to a session does not overwrite `saved`, `archived`, or `completed`
- user reply to a session does not mark the session inbox row `completed`
- explicit session completion marks the session inbox row `completed` without shutting down the session
- terminal session reporting stores resolved scope/headline on the completed turn
- session inbox rows expose the latest scope/headline preview
- scope is reused from sticky session metadata when omitted
- a supplied new scope updates sticky session metadata
- missing or malformed metadata falls back to a usable scope/headline
- blocker reporting updates the session and creates inbox attention
- declare-initiative leaves existing inbox rows untouched
- turn-ended reporting emits `session.turn_ended` only when no other attention entity was touched
- one-shot completion follows the same suppression rule
- `prSubmit` creates unread pull-request attention
- `prReply` refreshes unread pull-request attention on the existing pull-request entity
- `prReply` does not mark the PR inbox row `completed`
- observed PR merged or closed state marks the PR inbox row `completed` if PR lifecycle refresh is implemented in this phase

### SDK tests

- `sdk.pr.get(...)` forwards correctly
- `sdk.session.complete(...)` forwards correctly
- `sdk.inbox.list(...)` forwards correctly
- `sdk.inbox.update(...)` forwards correctly
- `sdk.inbox.bulkUpdate(...)` forwards correctly

## Merge Criteria

The feature is ready when all of the following are true:

- daemon store includes `inboxItems`
- `inboxItems.entityId` is unique
- daemon IPC includes pull-request get, inbox list, inbox update, inbox bulk update, session completion, and session reporting methods
- `goddard-tool` no longer mutates session records directly for initiative, blocker, or turn-ended reporting
- terminal session commands accept structured `scope` and `headline` metadata
- completed session turns preserve resolved inbox metadata
- session inbox rows expose latest scope/headline preview fields
- user replies mark active session inbox rows as `replied`, not `completed`
- explicit session completion is the only session path that marks a session inbox row `completed`
- PR submit and reply both emit inbox attention
- PR reply does not mark a PR inbox row `completed`
- SDK exposes the new inbox and PR methods
- automated tests cover single-item update, bulk update, deterministic touch conflict handling, status transitions, turn metadata, fallback behavior, and turn-suppression behavior
- the canonical `spec/` documentation has been updated to match the implemented behavior

## Risks To Watch During Implementation

- The unique `entityId` constraint protects the single-row-per-entity invariant; the manager still needs deterministic conflict handling when two writers race.
- Bulk updates must not accidentally assign per-row timestamps.
- `completed` must stay entity-lifecycle-specific; using it as another "handled" or "replied" state will make PR and session rows misleading.
- `replied` must be reopened by later daemon attention, or answered sessions can get stuck looking handled after new work appears.
- Session turn suppression logic should stay in one daemon-owned place, not spread across CLI, PR handlers, and one-shot paths.
- Keep the boundary clear between turn metadata and inbox state: scope/headline belongs to the turn preview model, while status/priority belongs to the inbox row.
- Fallback synthesis can become vague if it leans too heavily on generic session titles; tests should cover concrete blocker and PR cases.
