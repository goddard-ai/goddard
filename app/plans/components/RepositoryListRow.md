# Component: RepositoryListRow
- **Minimum Viable Component:** One managed repository row showing path, slug, detected config presence, and quick navigation actions.
- **Props Interface:** `repository: { id, path, slug, hasLocalConfig, hasActions, hasLoops, updatedAt }`; `isSelected: boolean`; `onSelect: (id) => void`; `onRemove: (id) => void`; `onOpenSpecs?: (id) => void`; `onOpenTasks?: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover and button visibility state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects the repository row; removes the repository; opens repository-scoped pages when requested.
