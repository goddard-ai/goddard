# Component: SpecTreeSidebar
- **Minimum Viable Component:** Left-hanging tree navigation for projects, spec folders, and page groupings relevant to specification management.
- **Props Interface:** `tree: array of project and directory nodes`; `selectedNodeId?: string | null`; `expandedNodeIds: array of string`; `onSelectNode: (id) => void`; `onToggleNode: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only expanded-node and focus state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Expands or collapses tree nodes; selects the active project or directory scope for the spec file list.
