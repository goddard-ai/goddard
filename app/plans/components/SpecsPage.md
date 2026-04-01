# Component: SpecsPage
- **Minimum Viable Component:** Full-width specification discovery page with a left-hanging project or path tree and a file list that opens MDX document tabs.
- **Props Interface:** `class?: string`; `embedded?: boolean`.
- **Sub-components:** `SpecTreeSidebar`, `SpecFileList`.
- **State Complexity:** Simple local empty-state and sidebar sizing; project content discovery belongs in `ProjectContentState`.
- **Required Context:** `ProjectContentContext`, `ProjectRegistryContext`, `WorkbenchTabsContext`.
- **Electrobun RPC:** None directly; file reads should route through `ProjectContentState`.
- **Interactions & Events:** Selects a project or directory node; filters visible spec files; opens a selected document in a detail tab.
