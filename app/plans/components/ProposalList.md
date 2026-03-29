# Component: ProposalList
- **Minimum Viable Component:** Prioritized list of roadmap proposals with click-through into proposal detail tabs.
- **Props Interface:** `proposals: array of proposal summary records`; `selectedProposalId?: string | null`; `onSelect: (id) => void`; `onOpen: (id) => void`.
- **Sub-components:** `ProposalListRow`.
- **State Complexity:** Simple UI-only keyboard focus and list virtualization state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects a proposal; opens the proposal detail tab; preserves current roadmap filters while navigating.
