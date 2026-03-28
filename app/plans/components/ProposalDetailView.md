# Component: ProposalDetailView
- **Minimum Viable Component:** Detail-tab view for one roadmap proposal with status, ownership, narrative, and linked work context.
- **Props Interface:** `proposalId: string`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple local section collapse and anchor navigation state; proposal data and mutations belong in `RoadmapState`.
- **Required Context:** `RoadmapContext`, `ActionCatalogContext`.
- **Tauri IPC:** None directly; proposal updates should route through roadmap state.
- **Interactions & Events:** Loads the selected proposal; edits status or priority; opens linked tasks, sessions, or pull requests; launches contextual actions.
