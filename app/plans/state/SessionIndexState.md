# State Module: SessionIndexState

- **Responsibility:** Own the recency-sorted directory of coding agent sessions and the bulk-selection workflows attached to that directory.
- **Data Shape:** One normalized session map keyed by daemon session id; one ordered array of ids sorted by `updatedAt` descending; one selection set; one query string; one status filter; pagination cursor metadata; loading and error state; derived UI labels so daemon `done` can render as user-facing `completed` without changing the shared schema contract.
- **Mutations/Actions:** `loadInitialSessions`; `loadMoreSessions`; `refreshSessions`; `mergeSessionUpdate`; `setQuery`; `setStatusFilter`; `toggleSelection`; `clearSelection`; `archiveSelectedSessions`; `openSessionChat`; `openSessionDiff`; `openSessionPullRequest`.
- **Scope & Hoisting:** Hoisted into a global provider because sessions are referenced from the main list, inbox links, and multiple detail tabs.
- **Side Effects:** Calls daemon or SDK session-list APIs; listens for realtime session updates and merges them into the ordered list; requests tab openings through `WorkbenchTabsState`; persists list preferences such as query and filter if that becomes part of workspace preferences.
