# Component: LoopsPage
- **Minimum Viable Component:** Full-width operator page for inspecting configured loops, filtering them by repository, and starting or stopping loop runtimes.
- **Props Interface:** `className?: string`; `embedded?: boolean`.
- **Sub-components:** `LoopFilterSidebar`, `LoopList`, `StartLoopDialog`.
- **State Complexity:** Simple local empty-state and sidebar sizing; runtime list and lifecycle actions belong in `LoopRuntimeState`.
- **Required Context:** `LoopRuntimeContext`, `RepositoryRegistryContext`.
- **Tauri IPC:** None directly; loop lifecycle calls should route through `LoopRuntimeState`.
- **Interactions & Events:** Filters loops; starts one loop; stops one loop; opens linked sessions when a running loop exposes them.
