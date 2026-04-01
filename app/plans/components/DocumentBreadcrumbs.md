# Component: DocumentBreadcrumbs
- **Minimum Viable Component:** Compact breadcrumb trail showing the managed project and path hierarchy for the active document.
- **Props Interface:** `segments: array of { id, label, path }`; `onOpenSegment?: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only overflow and hover state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Opens an ancestor path or project view when breadcrumb segments are clicked.
