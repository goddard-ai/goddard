# Component: InboxPage
- **Minimum Viable Component:** Gmail-like inbox view for coding agent updates with multi-select, hover actions, and bulk workflows.
- **Props Interface:** `class?: string`; `embedded?: boolean`.
- **Sub-components:** `InboxToolbar`, `InboxList`.
- **State Complexity:** Simple local empty-state and focus presentation; item loading, selection, snooze, archive, and delegate workflows belong in `InboxState`.
- **Required Context:** `InboxContext`, `WorkbenchTabsContext`.
- **Tauri IPC:** None.
- **Interactions & Events:** Refreshes inbox items; opens linked sessions or pull requests in tabs; bulk archives or snoozes selected items; delegates selected work back to an agent.
