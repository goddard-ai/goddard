# Component: SpecsPage
- **Minimum Viable Component:** Full-width specification discovery page with a left-hanging repository or path tree and a file list that opens MDX document tabs.
- **Props Interface:** `className?: string`; `embedded?: boolean`.
- **Sub-components:** `SpecTreeSidebar`, `SpecFileList`.
- **State Complexity:** Simple local empty-state and sidebar sizing; repository content discovery belongs in `RepositoryContentState`.
- **Required Context:** `RepositoryContentContext`, `RepositoryRegistryContext`, `WorkbenchTabsContext`.
- **Tauri IPC:** None directly; file reads should route through `RepositoryContentState`.
- **Interactions & Events:** Selects a repository or directory node; filters visible spec files; opens a selected document in a detail tab.
