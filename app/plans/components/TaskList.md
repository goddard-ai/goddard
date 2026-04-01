# Component: TaskList
- **Minimum Viable Component:** Prioritized task list with stable sorting and click-through into task detail tabs.
- **Props Interface:** `tasks: array of task summary records`; `selectedTaskId?: string | null`; `onSelect: (id) => void`; `onOpen: (id) => void`.
- **Sub-components:** `TaskListRow`.
- **State Complexity:** Simple UI-only keyboard focus and list virtualization state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects a task; opens the task detail tab; preserves list filters while navigating.
