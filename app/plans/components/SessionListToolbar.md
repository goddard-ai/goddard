# Component: SessionListToolbar
- **Minimum Viable Component:** Toolbar above the session list for search, status filtering, bulk archive, and summary counts.
- **Props Interface:** `query: string`; `statusFilter: string | null`; `selectedCount: number`; `totalCount: number`; `isRefreshing: boolean`; `onQueryChange: (value) => void`; `onStatusFilterChange: (value) => void`; `onArchiveSelected: () => void`; `onRefresh: () => void`; `onClearSelection: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only input focus and popover state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Typing updates the query; selecting a filter updates the visible set; clicking bulk archive emits one action for the selected ids; refresh requests a data reload.
