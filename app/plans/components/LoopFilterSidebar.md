# Component: LoopFilterSidebar

- **Minimum Viable Component:** Left-hanging sidebar for narrowing visible loop definitions and runtimes by project, state, and text query.
- **Props Interface:** `query: string`; `projectFilter: string | null`; `runtimeStateFilter: string | null`; `projects: array of project summaries`; `onQueryChange: (value) => void`; `onProjectFilterChange: (value) => void`; `onRuntimeStateFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only filter field state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Updates the loop list filters; narrows loop definitions to one project or runtime state.
