# Component: TasksPage
- **Minimum Viable Component:** Full-width task prioritization page that uses a list layout, not a board, with a left-hanging filter sidebar and task detail tabs.
- **Props Interface:** `class?: string`; `embedded?: boolean`.
- **Sub-components:** `TaskFilterSidebar`, `TaskList`.
- **State Complexity:** Simple local empty-state and sidebar sizing; task data, ordering, and filters belong in `TaskListState`.
- **Required Context:** `TaskListContext`, `ProjectRegistryContext`, `WorkbenchTabsContext`.
- **Electrobun RPC:** None directly; task reads and writes should route through shared task state.
- **Interactions & Events:** Filters and sorts tasks; opens a task detail tab; updates task status or priority through row or detail actions.
