# Component: RoadmapPage
- **Minimum Viable Component:** Full-width roadmap page that shows proposal records in a prioritized list layout with filtering and detail-tab drill-down.
- **Props Interface:** `class?: string`; `embedded?: boolean`.
- **Sub-components:** `ProposalFilterSidebar`, `ProposalList`.
- **State Complexity:** Simple local empty-state and sidebar sizing; proposal data and filtering belong in `RoadmapState`.
- **Required Context:** `RoadmapContext`, `ProjectRegistryContext`, `WorkbenchTabsContext`.
- **Electrobun RPC:** None directly; roadmap reads and writes should route through shared proposal state.
- **Interactions & Events:** Filters and sorts proposals; opens a proposal detail tab; updates proposal status or priority through list or detail actions.
