# Component: WorkbenchTabPanel
- **Minimum Viable Component:** Active content host that renders the view associated with the selected detail tab while respecting keep-alive or cached restoration rules.
- **Props Interface:** `activeTab: { id, kind, payload } | null`; `cachedTabs: array of { id, kind, payload, visible }`; `renderersByKind: record from tab kind to component`.
- **Sub-components:** `SessionChatView`, `PullRequestView`, `CodeDiffView`, `MdxDocumentView`, `ActionEditorView`, `TaskDetailView`, `ProposalDetailView`, `TerminalView`, `BrowserPreviewView`.
- **State Complexity:** Simple UI-only visibility switching; keep-alive and cache policy live in `WorkbenchTabsState`.
- **Required Context:** `WorkbenchTabsContext`.
- **Tauri IPC:** None.
- **Interactions & Events:** Mounts the correct tab view; toggles visibility without discarding cached state; forwards close and title update events from child views back into tab state.
