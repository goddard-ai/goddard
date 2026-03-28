# Component: TaskDetailView
- **Minimum Viable Component:** Detail-tab view for one task record with status, ownership, description, and linked work context.
- **Props Interface:** `taskId: string`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple local section collapse and anchor navigation state; task data and mutations belong in `TaskListState`.
- **Required Context:** `TaskListContext`, `ActionCatalogContext`.
- **Tauri IPC:** None directly; task updates should route through task state.
- **Interactions & Events:** Loads the selected task; edits status or assignment; opens linked sessions, pull requests, or specs; launches contextual actions.
