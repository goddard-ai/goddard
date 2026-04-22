# Component: ProposalListRow

- **Minimum Viable Component:** One roadmap proposal row showing project, title, status, owner, priority, and updated time.
- **Props Interface:** `proposal: { id, projectLabel, title, status, owner, priority, updatedAt }`; `isSelected: boolean`; `onSelect: (id) => void`; `onOpen: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover and pressed state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects the row; opens the proposal detail tab; makes the current roadmap ordering clear.
