# Component: ContextActionDropdown
- **Minimum Viable Component:** Filterable dropdown of global actions and context-applicable actions for the current tab or launch form.
- **Props Interface:** `currentTabContext?: { kind, repositoryId?, entityRef? } | null`; `mode: "global" | "contextual" | "combined"`; `selectedActionId?: string | null`; `placeholder?: string`; `onSelect: (actionId) => void`; `onOpenManageActions?: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only popover, query, and highlighted-option state; action resolution and applicability filtering belong in `ActionCatalogState`.
- **Required Context:** `ActionCatalogContext`.
- **Tauri IPC:** None.
- **Interactions & Events:** Filters actions by typed query; groups global and contextual actions; selects one action; optionally opens the full actions management page.
