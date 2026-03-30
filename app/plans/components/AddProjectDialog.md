# Component: AddProjectDialog
- **Minimum Viable Component:** Modal flow for selecting one existing local directory and adding it to the app’s managed project registry.
- **Props Interface:** `isOpen: boolean`; `draft: { path, name }`; `validation: { canSubmit, errorMessage?, validatedProject?: { path, name } }`; `onBrowse: () => void`; `onPathChange: (value) => void`; `onNameChange: (value) => void`; `onValidate: () => void`; `onSubmit: () => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only modal and validation display state; the persisted project list belongs in `ProjectRegistry`.
- **Required Context:** `ProjectRegistryContext`.
- **Tauri IPC:** None directly; path selection should route through project-registry service adapters.
- **Interactions & Events:** Opens a directory picker; validates the selected path; adds the project to the registry; closes on success or cancellation.
