# Component: WorkbenchTabs
- **Minimum Viable Component:** Top tab strip and tab body host for closable detail tabs, with reorder support, hover-only close affordances, and active-tab rendering.
- **Props Interface:** `primaryTab: { id, title, icon }`; `detailTabs: array of { id, kind, title, icon, closable, dirty?, statusBadge? }`; `activeTabId: string`; `detailTabLimit: number`; `onSelect: (id) => void`; `onClose: (id) => void`; `onReorder: (fromId, toId) => void`.
- **Sub-components:** `WorkbenchTab`, `WorkbenchTabPanel`.
- **State Complexity:** Simple local drag-preview and hover state only; tab identity, LRU eviction, persistence, and cached restoration belong in `WorkbenchTabsState`.
- **Required Context:** `WorkbenchTabsContext` when connected; otherwise none.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects tabs; reveals close buttons on hover; emits reorder events during drag and drop; requests closure; displays when the configured detail-tab limit has been reached.
