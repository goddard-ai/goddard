# Component: TaskFilterSidebar
- **Minimum Viable Component:** Left-hanging sidebar for filtering tasks by project, status, owner, and text query.
- **Props Interface:** `query: string`; `projectFilter: string | null`; `statusFilter: string | null`; `ownerFilter: string | null`; `projects: array of project summaries`; `owners: array of string`; `onQueryChange: (value) => void`; `onProjectFilterChange: (value) => void`; `onStatusFilterChange: (value) => void`; `onOwnerFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only input and disclosure state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Adjusts task list filters; narrows visible tasks to one project or assignee; clears filters back to the default prioritized view.
