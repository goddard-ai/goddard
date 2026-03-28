# Component: TaskFilterSidebar
- **Minimum Viable Component:** Left-hanging sidebar for filtering tasks by repository, status, owner, and text query.
- **Props Interface:** `query: string`; `repositoryFilter: string | null`; `statusFilter: string | null`; `ownerFilter: string | null`; `repositories: array of managed repository summaries`; `owners: array of string`; `onQueryChange: (value) => void`; `onRepositoryFilterChange: (value) => void`; `onStatusFilterChange: (value) => void`; `onOwnerFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only input and disclosure state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Adjusts task list filters; narrows visible tasks to one repository or assignee; clears filters back to the default prioritized view.
