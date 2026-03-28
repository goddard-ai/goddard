# Component: ActionFilterSidebar
- **Minimum Viable Component:** Left-hanging sidebar for filtering actions by scope, repository, applicability, and text query.
- **Props Interface:** `query: string`; `scopeFilter: "all" | "global" | "repository"`; `repositoryFilter: string | null`; `applicabilityFilter: "all" | "current-tab" | "always"`; `repositories: array of managed repository summaries`; `onQueryChange: (value) => void`; `onScopeFilterChange: (value) => void`; `onRepositoryFilterChange: (value) => void`; `onApplicabilityFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only field focus and collapsed group state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Updates filters for the action catalog; narrows the list to actions relevant to one repository or the current tab context.
