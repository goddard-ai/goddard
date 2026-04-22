# Component: ProposalFilterSidebar

- **Minimum Viable Component:** Left-hanging sidebar for filtering roadmap proposals by project, status, owner, and text query.
- **Props Interface:** `query: string`; `projectFilter: string | null`; `statusFilter: string | null`; `ownerFilter: string | null`; `projects: array of project summaries`; `owners: array of string`; `onQueryChange: (value) => void`; `onProjectFilterChange: (value) => void`; `onStatusFilterChange: (value) => void`; `onOwnerFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only filter control state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Adjusts proposal filters; narrows the roadmap list to one project or owner; clears filters back to the default prioritization view.
