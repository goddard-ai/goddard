# Component: ActionsPage
- **Minimum Viable Component:** Full-width management page for browsing, filtering, creating, and opening reusable actions.
- **Props Interface:** `className?: string`; `embedded?: boolean`.
- **Sub-components:** `ActionFilterSidebar`, `ActionList`, `CreateActionDialog`.
- **State Complexity:** Simple local empty-state and splitter sizing; catalog filtering belongs in `ActionCatalogState`, while create and edit drafts belong in `ActionDraftState`.
- **Required Context:** `ActionCatalogContext`, `ActionDraftContext`, `WorkbenchTabsContext`.
- **Tauri IPC:** None directly; action persistence should route through action state modules and shared config adapters.
- **Interactions & Events:** Filters action lists; opens one action in an editor tab; creates a new action; refreshes repository and global action sources.
