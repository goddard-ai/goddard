# Component: ProjectListRow
- **Minimum Viable Component:** One managed project row showing path, display name, and quick navigation actions.
- **Props Interface:** `project: { path, name }`; `isSelected: boolean`; `onSelect: (path) => void`; `onRemove: (path) => void`; `onOpenProjectTab?: (path) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover and button visibility state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects the project row; removes the project; opens project-scoped pages when requested.
