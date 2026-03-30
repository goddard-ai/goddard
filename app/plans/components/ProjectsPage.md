# Component: ProjectsPage
- **Minimum Viable Component:** Full-width project management page for adding, removing, and inspecting local project roots the app should manage across the current machine.
- **Props Interface:** `class?: string`; `embedded?: boolean`.
- **Sub-components:** `ProjectList`, `AddProjectDialog`.
- **State Complexity:** Simple local empty-state and selection presentation; project registry state belongs in `ProjectRegistry`; add-flow validation belongs in localized UI state plus query state.
- **Required Context:** `ProjectRegistryContext`.
- **Tauri IPC:** None directly; file-picker and project validation flows should route through `ProjectRegistry`-adjacent adapters.
- **Interactions & Events:** Opens the add-project dialog; removes projects from the managed set; opens project detail tabs.
