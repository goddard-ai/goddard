# Component: SessionsPage
- **Minimum Viable Component:** Primary view that lists coding agent sessions in descending recency order and exposes chat, diff, PR, and bulk archive workflows.
- **Props Interface:** `className?: string`; `embedded?: boolean`.
- **Sub-components:** `SessionListToolbar`, `SessionList`.
- **State Complexity:** Simple local focus and empty-state presentation; data loading, sorting, selection, and tab opening stay in `SessionIndexState`.
- **Required Context:** `SessionIndexContext`, `WorkbenchTabsContext`.
- **Tauri IPC:** None.
- **Interactions & Events:** Refreshes sessions; opens an existing or new chat tab on row click; opens diff and PR tabs from row actions; archives selected sessions from the toolbar.
