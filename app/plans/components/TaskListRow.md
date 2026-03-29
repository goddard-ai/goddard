# Component: TaskListRow
- **Minimum Viable Component:** One task row showing repository, title, status, owner, priority, and updated time.
- **Props Interface:** `task: { id, repositoryLabel, title, status, owner, priority, updatedAt }`; `isSelected: boolean`; `onSelect: (id) => void`; `onOpen: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover and pressed state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects the row; opens the task detail tab; exposes the prioritized list ordering clearly.
