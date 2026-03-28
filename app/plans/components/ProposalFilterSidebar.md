# Component: ProposalFilterSidebar
- **Minimum Viable Component:** Left-hanging sidebar for filtering roadmap proposals by repository, status, owner, and text query.
- **Props Interface:** `query: string`; `repositoryFilter: string | null`; `statusFilter: string | null`; `ownerFilter: string | null`; `repositories: array of managed repository summaries`; `owners: array of string`; `onQueryChange: (value) => void`; `onRepositoryFilterChange: (value) => void`; `onStatusFilterChange: (value) => void`; `onOwnerFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only filter control state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Adjusts proposal filters; narrows the roadmap list to one repository or owner; clears filters back to the default prioritization view.
