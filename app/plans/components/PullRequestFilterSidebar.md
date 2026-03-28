# Component: PullRequestFilterSidebar
- **Minimum Viable Component:** Left-hanging sidebar that filters pull requests by repository, state, author, and managed status.
- **Props Interface:** `query: string`; `repositoryFilter: string | null`; `statusFilter: string | null`; `authorFilter: string | null`; `managedFilter: "all" | "managed" | "unmanaged"`; `repositories: array of managed repository summaries`; `authors: array of string`; `onQueryChange: (value) => void`; `onRepositoryFilterChange: (value) => void`; `onStatusFilterChange: (value) => void`; `onAuthorFilterChange: (value) => void`; `onManagedFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only checkbox, select, and focus state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Adjusts the visible PR list; narrows the list to one repository or author; filters to managed pull requests only when desired.
