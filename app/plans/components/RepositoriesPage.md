# Component: RepositoriesPage
- **Minimum Viable Component:** Full-width repository management page for adding, removing, and inspecting repositories the app should manage across the current machine.
- **Props Interface:** `className?: string`; `embedded?: boolean`.
- **Sub-components:** `RepositoryList`, `AddRepositoryDialog`.
- **State Complexity:** Simple local empty-state and loading presentation; repository registry state belongs in `RepositoryRegistryState`.
- **Required Context:** `RepositoryRegistryContext`.
- **Tauri IPC:** None directly; file-picker and repository inspection flows should route through `RepositoryRegistryState`.
- **Interactions & Events:** Opens the add-repository dialog; removes repositories from the managed set; refreshes repository capability metadata.
