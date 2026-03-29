# Component: RepositoryList
- **Minimum Viable Component:** List of managed repositories with status, detected capabilities, and quick actions.
- **Props Interface:** `repositories: array of managed repository records`; `selectedRepositoryId?: string | null`; `onSelect: (id) => void`; `onRemove: (id) => void`; `onOpenSpecs?: (id) => void`; `onOpenTasks?: (id) => void`.
- **Sub-components:** `RepositoryListRow`.
- **State Complexity:** Simple UI-only selection and empty-state handling.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects one repository; removes it from the registry; opens a relevant page already filtered to that repository when needed.
