# Component: LoopFilterSidebar
- **Minimum Viable Component:** Left-hanging sidebar for narrowing visible loop definitions and runtimes by repository, state, and text query.
- **Props Interface:** `query: string`; `repositoryFilter: string | null`; `runtimeStateFilter: string | null`; `repositories: array of managed repository summaries`; `onQueryChange: (value) => void`; `onRepositoryFilterChange: (value) => void`; `onRuntimeStateFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only filter field state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Updates the loop list filters; narrows loop definitions to one repository or runtime state.
