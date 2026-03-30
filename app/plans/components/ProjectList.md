# Component: ProjectList
- **Minimum Viable Component:** List of managed projects with stable identity and quick actions.
- **Props Interface:** `projects: array of project records`; `selectedProjectPath?: string | null`; `onSelect: (path) => void`; `onRemove: (path) => void`; `onOpenProjectTab?: (path) => void`.
- **Sub-components:** `ProjectListRow`.
- **State Complexity:** Simple UI-only selection and empty-state handling.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects one project; removes it from the registry; opens a relevant project tab when needed.
