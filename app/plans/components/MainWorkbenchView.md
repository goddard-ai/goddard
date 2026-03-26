# Component: MainWorkbenchView
- **Minimum Viable Component:** Non-closable primary workbench surface whose content changes when the selected sidebar domain changes.
- **Props Interface:** `selectedNavId: string`; `mainTabId: string`; `emptyState?: { title, body }`.
- **Sub-components:** `SessionsPage`, `InboxPage`, `MdxDocumentView`.
- **State Complexity:** Simple UI-only routing logic; feature state stays inside domain modules.
- **Required Context:** `NavigationContext`; feature contexts consumed indirectly by whichever primary view is active.
- **Tauri IPC:** None.
- **Interactions & Events:** Reacts to navigation selection changes; preserves per-view scroll and focus when the primary view changes; can emit “open detail tab” requests through child callbacks.
