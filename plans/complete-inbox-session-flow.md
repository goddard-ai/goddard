# Complete Inbox Session Flow

## Goal

Complete the UI flow where a human creates a session, the agent ends a turn with `goddard end-turn`, the app surfaces an unread inbox item immediately, the human opens and replies to the session from the inbox, and the human can later mark the session completed so the inbox item moves to Completed and the session leaves the primary session list.

## Current State

- The daemon and SDK already expose inbox primitives:
  - `goddard end-turn` calls `session.reportTurnEnded`.
  - `session.reportTurnEnded` creates or refreshes a session inbox row as `unread`.
  - `inbox.list`, `inbox.update`, and `inbox.bulkUpdate` exist.
  - Session prompt submission calls `markSessionReplied`.
  - `session.complete` marks the session inbox item as `completed`.
- The app has no real Inbox page. `workbench-tab-registry.ts` still maps `inbox` to `PlaceholderWorkbenchTab`.
- The app has no unread inbox badge in the sidebar.
- The daemon has no inbox stream. App subscriptions currently cover `session.message` and `workforce.event`, so inbox freshness needs either polling or a new `inbox.item`/activity stream.
- `session.complete` does not yet hide completed sessions from the primary Sessions list or support restoring them to the list on a later human reply.
- The session chat header has no "mark completed" action.

## Product Decisions

- Treat `Inbox Item` as the workflow record. The UI should group by `status`, not infer status from session state.
- Keep the first implementation narrow:
  - Show session-linked inbox items.
  - Preserve PR-linked items in daemon storage, but hide them from the first Inbox page until the app has supported PR row actions.
  - Implement sections for unread/read attention, replied, and completed.
- "Instantly reflects" should mean the UI updates without a manual refresh. Prefer a daemon-published inbox stream; use short polling only as a temporary implementation if the stream would block the flow.
- Completing from the session tab should be one user action that:
  - marks the associated inbox item `completed`;
  - hides the session from the default Sessions list;
  - keeps the session readable and interactive.
- A completed session is not terminal. If the human sends another message in a completed session, the associated inbox item should move to `replied` and the session should return to the default Sessions list.

## Implementation Plan

### 1. Add Realtime Inbox Updates

- Product ambiguity: Resolved.
- Extend `core/schema/src/daemon-ipc.ts` with an inbox stream such as `inbox.item`.
- Define a stream payload that can carry the changed `InboxItem` and the mutation kind.
- Treat the stream as the source of cross-surface freshness for the inbox page, sidebar unread badge, and any already-open session tab. A new or changed inbox item should be visible in those surfaces without requiring a manual refresh or navigation away/back.
- Scope the first version to daemon-local in-app notification surfaces. It should update the inbox page and sidebar unread badge only; it should not request OS notification permission, send desktop notifications, provide cross-device sync, cloud push, or deliver notifications while the app and daemon are both stopped.
- Publish this stream from all daemon inbox writes:
  - `touchInboxItem`;
  - `updateInboxItem`;
  - `bulkUpdateInboxItems`;
  - `markSessionReplied`;
  - `completeSession`.
- If multiple updates for the same item arrive quickly, the UI should settle on the latest item status and metadata instead of showing duplicate rows.
- Keep stream publishing outside the schema-only inbox manager if that keeps persistence test setup simple. A small daemon-owned publisher wrapper is preferable to making the persistence manager depend on IPC.
- Add daemon tests that verify `session.reportTurnEnded`, `inbox.update`, prompt reply, and `session.complete` produce the expected stream payloads or refresh hooks.

### 2. Complete Session Completion Semantics

- Product ambiguity: Resolved.
- Add a daemon operation that represents the UI action, either by changing `session.complete` or adding a more explicit endpoint.
- The operation should:
  - require the session to exist;
  - reject completion while the agent has an active turn;
  - mark the inbox row `completed`;
  - hide the session from the default Sessions list;
  - preserve readable session history;
  - preserve the ability to send another message from an already-open or reopened session tab.
- Completed is a reversible workflow state, not a terminal session state. Because completion is unavailable while the agent is active, renewed work starts with a human reply. When the human sends another message in a completed session, the inbox item should move to `replied` and the session should reappear in the default Sessions list. If the agent later ends another turn, the existing end-turn behavior should move the item back to `unread`.
- Update `session.list` to exclude completed-hidden sessions by default while still allowing direct `session.get`, history reads, and prompt submission for those sessions.
- Add an explicit filter only if the app needs a Completed or hidden Sessions view now; otherwise keep discovery through the Inbox Completed section.
- Add tests for:
  - completed session row moves to inbox `completed`;
  - completion is unavailable or rejected while a turn is active;
  - default session list omits completed-hidden sessions;
  - direct `session.get`, history reads, and prompt submission still work for completed-hidden sessions;
  - sending a human reply moves the inbox row to `replied` and returns the session to the default Sessions list;
  - a later agent turn end moves the restored session item back to `unread`.

### 3. Add App Inbox State

- Product ambiguity: Resolved.
- Create an app inbox feature under `app/src/inbox/`.
- Use the existing query cache for daemon reads and a small Sigma owner only if needed for cross-shell state such as unread counts and stream merging.
- Initial reads:
  - query `unread` and `read` for the attention section;
  - query `replied`;
  - query `completed`.
- Subscribe to the new inbox stream and merge changed items into the local section state.
- Invalidate or refetch affected inbox queries on stream events to stay correct if pagination or section membership changes.
- Derive sidebar unread count from `unread` items, not from session status.
- A changed item should appear in exactly one status section at a time. If an item changes from `completed` to `replied` or `replied` to `unread`, remove it from the previous section during the same visible update.
- Preserve user trust during daemon disconnects by keeping the last loaded inbox state visible and showing a compact stale/error indication instead of clearing the list.

### 4. Build the Inbox Surface

- Product ambiguity: Resolved.
- Replace the `inbox` placeholder in `app/src/workbench-tab-registry.ts` with a lazy `InboxPage`.
- Implement:
  - `InboxPage`;
  - `InboxList`;
  - `InboxRow`;
  - minimal styles matching the existing app shell and session list density.
- Show only session-linked inbox items in the first visible Inbox page. PR-linked inbox rows may continue to exist in daemon storage, but they should be hidden from this surface until the app has a supported PR row action.
- Sections:
  - `Needs attention`: `unread` and `read`;
  - `Replied`: `replied`;
  - `Completed`: `completed`.
- Row content:
  - scope;
  - headline;
  - reason label;
  - updated time;
  - unread indicator on unread rows.
- Empty states should be compact and section-local.
- Empty states should reflect visible session items only. If the daemon has hidden non-session inbox rows but no visible session rows, the page should still read as empty for this first session-focused version.

### 5. Wire Inbox Row Navigation

- Product ambiguity: Resolved.
- Marking an inbox item read is an associated-entity visit behavior, not an inbox-specific click behavior. When the user visits the session associated with an unread session inbox item from any path, the app should mark that inbox item `read`.
- For session items, opening the row should:
  - open or focus the session chat detail tab;
  - rely on the session visit behavior to mark the item `read` if it is currently `unread`;
  - preserve the session tab behavior already used from the Sessions list.
- Session chat loading should trigger the read mutation only after the associated session is successfully available to the user. Failed navigation or failed session loading should preserve the unread inbox item.
- The same read-on-visit rule should apply when the user opens the session from the Sessions list, a restored tab, command navigation, or another future entity link.
- After marking read from the entity visit, update:
  - the row status locally;
  - unread badge count;
  - inbox queries.
- If session loading fails, surface the error in the inbox row or a compact page-level error.

### 6. Add Sidebar Unread Badge

- Product ambiguity: Resolved.
- Extend `AppShellChrome` sidebar rendering to accept per-navigation badges.
- Render a small blue dot over the Inbox button when at least one visible session inbox item is unread. Do not show a numeric count in the sidebar.
- Keep the badge accessible:
  - include unread presence in the Inbox button `aria-label`;
  - use the dot as visual presentation only.
- Update badge state from the app inbox state or from a lightweight unread-count query plus stream merge.

### 7. Mark Completed From Session Chat

- Product ambiguity: Resolved.
- Add a session chat header action with a check icon and tooltip.
- The action does not require an existing inbox item to be visible or loaded, though normal end-turn flow is expected to create one.
- Completion availability depends on session type:
  - For worktree-based sessions, hide or disable the Complete action when the agent is active, the working tree is dirty, or there are unmerged commits.
  - For local sessions, hide or disable the Complete action only while the agent is active.
- When completion is unavailable because of worktree state, explain the blocking condition in the tooltip or inline error using product language such as "Resolve or commit changes before completing this worktree session."
- On click:
  - call the completion endpoint;
  - update the loaded session to completed presentation without disabling the composer;
  - invalidate session list, session get/history, and inbox queries;
  - keep the current session tab open so the human can still inspect history.
- Avoid adding a confirmation dialog because completion only hides the session from the primary list and remains reversible through continued interaction.
- If the daemon rejects completion after the button was shown, keep the session tab open and show a compact error explaining why completion is not currently available.

### 8. Tests And Verification

- Product ambiguity: Resolved.
- Daemon tests:
  - inbox stream publishing;
  - completion hides the session from the primary list and completes the inbox item;
  - completion is rejected while the agent is active;
  - worktree-based completion is unavailable when the working tree is dirty or has unmerged commits;
  - default session list excludes completed-hidden sessions;
  - replying in a completed-hidden session restores it to the primary list and moves its inbox item to `replied`.
- SDK/schema tests:
  - stream type coverage if schema tests exist for daemon IPC.
- App tests:
  - inbox page groups statuses correctly;
  - visiting a session associated with an unread inbox item marks it read, including when opened from outside the Inbox page;
  - hidden PR-linked inbox items do not appear in the first Inbox page;
  - submitting a prompt moves the item to Replied after query refresh/stream merge;
  - completing from chat moves the item to Completed and removes the session from the primary list;
  - replying in a completed session moves the item to Replied and returns the session to the primary list.
- Manual verification:
  - create a session;
  - run `goddard end-turn --scope "<scope>" --headline "<headline>"`;
  - confirm inbox badge appears without manual refresh;
  - open the inbox row and confirm it becomes read;
  - reply in chat and confirm it moves to Replied;
  - end another turn and confirm it returns to Needs attention/unread;
  - mark completed from chat and confirm it appears under Completed and disappears from Sessions.
  - reply in the completed session and confirm it moves to Replied and returns to Sessions.

## Suggested File Areas

- `core/schema/src/daemon-ipc.ts`
- `core/schema/src/daemon/inbox.ts`
- `core/daemon/src/inbox/manager.ts`
- `core/daemon/src/ipc/server.ts`
- `features/session/src/daemon/manager.ts`
- `core/sdk/src/sdk.ts`
- `app/src/inbox/*`
- `app/src/workbench-tab-registry.ts`
- `app/src/app-shell/chrome.tsrx`
- `app/src/app-shell/chrome.style.ts`
- `app/src/session-chat/view.tsrx`
- `app/src/sessions/mutations.ts`

## Risks

- If inbox stream publishing is bolted directly into persistence helpers, tests and future daemon contexts may become harder to isolate.
- If completed-hidden filtering is added only in the app, SDK consumers and future UI surfaces may still show completed sessions incorrectly.
- If completion behaves like a terminal archive instead of a reversible workflow state, users will lose the ability to continue work from a completed session that they intentionally reopened.
- If completion does not preserve history reads and prompt submission, the completed session tab will feel broken immediately after the user clicks the action.
- If the badge reads from a separately polled count while the inbox page uses cached lists, the sidebar and inbox page can drift.
