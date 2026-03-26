# State Module: InboxState
- **Responsibility:** Own the Gmail-like inbox of coding agent updates, including normalization, selection, snoozing, archiving, and delegation workflows.
- **Data Shape:** One normalized inbox item map keyed by inbox id; one ordered array grouped or sorted by most recently updated first; one selection set; one query string; one active filter; per-item linkage to session ids, pull request refs, diff refs, or repository refs; loading, error, and optimistic action state.
- **Mutations/Actions:** `loadInbox`; `mergeInboxEvent`; `setQuery`; `setFilter`; `toggleSelection`; `clearSelection`; `snoozeItems`; `archiveItems`; `delegateItems`; `openInboxItem`.
- **Scope & Hoisting:** Hoisted into a global provider because inbox items can update badge counts, open workbench tabs, and receive events while the user is on another view.
- **Side Effects:** Consumes normalized activity events from `RealtimeActivityState`; persists view preferences such as the last active filter; triggers SDK or daemon-backed delegation actions; routes “open” actions through `WorkbenchTabsState`.
