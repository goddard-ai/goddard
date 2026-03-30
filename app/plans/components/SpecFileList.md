# Component: SpecFileList
- **Minimum Viable Component:** Project-scoped list of spec and page documents that can be opened into MDX detail tabs.
- **Props Interface:** `files: array of { id, title, path, projectLabel, updatedAt, kind }`; `selectedFileId?: string | null`; `onSelect: (id) => void`; `onOpen: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only sort and keyboard focus state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects one file; opens it into an MDX document tab; keeps the current tree filter intact.
