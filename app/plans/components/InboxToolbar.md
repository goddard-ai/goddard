# Component: InboxToolbar

- **Minimum Viable Component:** Toolbar for inbox search, filter controls, selection summary, and bulk actions.
- **Props Interface:** `query: string`; `selectedCount: number`; `totalCount: number`; `activeFilter: string | null`; `onQueryChange: (value) => void`; `onFilterChange: (value) => void`; `onSnoozeSelected: () => void`; `onArchiveSelected: () => void`; `onDelegateSelected: () => void`; `onClearSelection: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only menu and input state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Updates the query and filter; triggers bulk snooze, archive, or delegate actions; clears the current selection.
