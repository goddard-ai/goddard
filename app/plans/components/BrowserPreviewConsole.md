# Component: BrowserPreviewConsole
- **Minimum Viable Component:** Inspectable log pane for console messages captured from the preview iframe through the injected protocol shim.
- **Props Interface:** `entries: array of { id, level, text, timestamp, sourceUrl?, line?, column? }`; `levelFilter: string | null`; `onLevelFilterChange: (level) => void`; `onClear: () => void`; `onSelectEntry?: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only filter chip and row expansion state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Filters logs by level; clears the visible console list; selects an entry to inspect its source metadata.
