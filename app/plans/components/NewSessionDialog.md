# Component: NewSessionDialog
- **Minimum Viable Component:** Modal-only session launch flow for starting one new coding agent session with repository, action, and initial prompt context.
- **Props Interface:** `isOpen: boolean`; `defaultRepositoryId?: string | null`; `defaultActionId?: string | null`; `currentTabContext?: { kind, repositoryId?, entityRef? } | null`; `onClose: () => void`; `onSubmitted: (sessionId) => void`.
- **Sub-components:** `SessionLaunchForm`, `ContextActionDropdown`.
- **State Complexity:** Simple UI-only focus trap and step transitions; launch form and submission state belong in `SessionLaunchState`.
- **Required Context:** `SessionLaunchContext`, `RepositoryRegistryContext`, `ActionCatalogContext`.
- **Tauri IPC:** None directly; session creation should route through `SessionLaunchState`.
- **Interactions & Events:** Opens from the sessions page or a contextual action; edits launch inputs; submits a new session request; closes on success or cancellation.
