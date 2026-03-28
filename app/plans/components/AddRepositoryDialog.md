# Component: AddRepositoryDialog
- **Minimum Viable Component:** Modal flow for selecting one local repository root and adding it to the app’s managed repository registry.
- **Props Interface:** `isOpen: boolean`; `draft: { path, displayName? }`; `validation: { canSubmit, fieldErrors }`; `inspection: { isInspecting, detectedSlug?, isGitRepository?, hasExistingConfig? }`; `onBrowse: () => void`; `onDraftChange: (patch) => void`; `onSubmit: () => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only modal and validation display state; browsing and inspection belong in `RepositoryRegistryState`.
- **Required Context:** `RepositoryRegistryContext`.
- **Tauri IPC:** None directly; path selection should route through `RepositoryRegistryState`.
- **Interactions & Events:** Opens a directory picker; validates the selected path; adds the repository to the registry; closes on success or cancellation.
