# Component: ActionFilterSidebar

- **Minimum Viable Component:** Left-hanging sidebar for filtering actions by scope, project, applicability, and text query.
- **Props Interface:** `query: string`; `scopeFilter: "all" | "global" | "project"`; `projectFilter: string | null`; `applicabilityFilter: "all" | "current-tab" | "always"`; `projects: array of project summaries`; `onQueryChange: (value) => void`; `onScopeFilterChange: (value) => void`; `onProjectFilterChange: (value) => void`; `onApplicabilityFilterChange: (value) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only field focus and collapsed group state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Updates filters for the action catalog; narrows the list to actions relevant to one project or the current tab context.
