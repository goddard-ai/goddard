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
- `session.complete` does not archive the session, and `session.list` does not filter archived sessions.
- The session chat header has no "mark completed" action.

## Product Decisions

- Treat `Inbox Item` as the workflow record. The UI should group by `status`, not infer status from session state.
- Keep the first implementation narrow:
  - Show session-linked inbox items.
  - Preserve PR-linked items in the data model, but render them as unsupported rows or route them only if existing PR UI is available.
  - Implement sections for unread/read attention, replied, and completed.
- "Instantly reflects" should mean the UI updates without a manual refresh. Prefer a daemon-published inbox stream; use short polling only as a temporary implementation if the stream would block the flow.
- Completing from the session tab should be one user action that:
  - marks the associated inbox item `completed`;
  - archives the daemon session;
  - removes the session from the default Sessions list.

## Implementation Plan

### 1. Add Realtime Inbox Updates

- Extend `core/schema/src/daemon-ipc.ts` with an inbox stream such as `inbox.item`.
- Define a stream payload that can carry the changed `InboxItem` and the mutation kind.
- Publish this stream from all daemon inbox writes:
  - `touchInboxItem`;
  - `updateInboxItem`;
  - `bulkUpdateInboxItems`;
  - `markSessionReplied`;
  - `completeSession`.
- Keep stream publishing outside the schema-only inbox manager if that keeps persistence test setup simple. A small daemon-owned publisher wrapper is preferable to making the persistence manager depend on IPC.
- Add daemon tests that verify `session.reportTurnEnded`, `inbox.update`, prompt reply, and `session.complete` produce the expected stream payloads or refresh hooks.

### 2. Complete Session Completion Semantics

- Add a daemon operation that represents the UI action, either by changing `session.complete` or adding a more explicit endpoint.
- The operation should:
  - require the session to exist;
  - mark the inbox row `completed`;
  - update the session status to `archived`;
  - preserve readable session history.
- Update `session.list` to exclude archived sessions by default.
- Add an explicit filter only if the app needs an Archived Sessions view now; otherwise keep the primary list simple.
- Add tests for:
  - completed session row moves to inbox `completed`;
  - session status becomes `archived`;
  - default session list omits archived sessions;
  - direct `session.get` and history reads still work for archived sessions.

### 3. Add App Inbox State

- Create an app inbox feature under `app/src/inbox/`.
- Use the existing query cache for daemon reads and a small Sigma owner only if needed for cross-shell state such as unread counts and stream merging.
- Initial reads:
  - query `unread` and `read` for the attention section;
  - query `replied`;
  - query `completed`.
- Subscribe to the new inbox stream and merge changed items into the local section state.
- Invalidate or refetch affected inbox queries on stream events to stay correct if pagination or section membership changes.
- Derive sidebar unread count from `unread` items, not from session status.

### 4. Build the Inbox Surface

- Replace the `inbox` placeholder in `app/src/workbench-tab-registry.ts` with a lazy `InboxPage`.
- Implement:
  - `InboxPage`;
  - `InboxList`;
  - `InboxRow`;
  - minimal styles matching the existing app shell and session list density.
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

### 5. Wire Inbox Row Navigation

- For session items, opening the row should:
  - mark the item `read` if it is currently `unread`;
  - open or focus the session chat detail tab;
  - preserve the session tab behavior already used from the Sessions list.
- After marking read, update:
  - the row status locally;
  - unread badge count;
  - inbox queries.
- If session loading fails, surface the error in the inbox row or a compact page-level error.

### 6. Add Sidebar Unread Badge

- Extend `AppShellChrome` sidebar rendering to accept per-navigation badges.
- Render a small blue dot over the Inbox button when unread count is greater than zero.
- Keep the badge accessible:
  - include unread count in the Inbox button `aria-label`;
  - use the dot as visual presentation only.
- Update badge state from the app inbox state or from a lightweight unread-count query plus stream merge.

### 7. Mark Completed From Session Chat

- Add a session chat header action with a check icon and tooltip.
- Show it when the session is in a human-reviewable state, especially `done`, `blocked`, or `replied` inbox state if available.
- On click:
  - call the completion endpoint;
  - update the loaded session to archived/completed presentation;
  - invalidate session list, session get/history, and inbox queries;
  - keep the current session tab open so the human can still inspect history.
- Avoid adding a confirmation dialog unless completion becomes destructive beyond archiving the primary list row.

### 8. Tests And Verification

- Daemon tests:
  - inbox stream publishing;
  - completion archives the session and completes inbox item;
  - default session list excludes archived sessions.
- SDK/schema tests:
  - stream type coverage if schema tests exist for daemon IPC.
- App tests:
  - inbox page groups statuses correctly;
  - opening an unread session row marks it read and opens the session tab;
  - submitting a prompt moves the item to Replied after query refresh/stream merge;
  - completing from chat moves the item to Completed and removes the session from the primary list.
- Manual verification:
  - create a session;
  - run `goddard end-turn --scope "<scope>" --headline "<headline>"`;
  - confirm inbox badge appears without manual refresh;
  - open the inbox row and confirm it becomes read;
  - reply in chat and confirm it moves to Replied;
  - end another turn and confirm it returns to Needs attention/unread;
  - mark completed from chat and confirm it appears under Completed and disappears from Sessions.

## Suggested File Areas

- `core/schema/src/daemon-ipc.ts`
- `core/schema/src/daemon/inbox.ts`
- `core/daemon/src/inbox/manager.ts`
- `core/daemon/src/ipc/server.ts`
- `core/daemon/src/session/manager.ts`
- `core/sdk/src/sdk.ts`
- `app/src/inbox/*`
- `app/src/workbench-tab-registry.ts`
- `app/src/app-shell/chrome.tsrx`
- `app/src/app-shell/chrome.style.ts`
- `app/src/session-chat/view.tsrx`
- `app/src/sessions/mutations.ts`

## Risks

- If inbox stream publishing is bolted directly into persistence helpers, tests and future daemon contexts may become harder to isolate.
- If archived filtering is added only in the app, SDK consumers and future UI surfaces may still show completed sessions incorrectly.
- If completion does not preserve history reads, the completed session tab will feel broken immediately after the user clicks the action.
- If the badge reads from a separately polled count while the inbox page uses cached lists, the sidebar and inbox page can drift.
