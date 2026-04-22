# Component: SessionListRow

- **Minimum Viable Component:** One recency-sorted session row that shows repository identity, generated title, raw status with UI label mapping, updated time, initiative or blocker text, and quick actions.
- **Props Interface:** `session: { id, repositoryHost, repositorySlug, title, rawStatus, displayStatusLabel, updatedAt, initiative, blockedReason }`; `isSelected: boolean`; `onToggleSelected: (id) => void`; `onOpenChat: (id) => void`; `onOpenDiff: (id) => void`; `onOpenPullRequest: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover state for action visibility.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Checkbox toggles selection; clicking the row opens the chat tab; hovering reveals quick actions; action buttons open recent diff or recent PR tabs without changing selection semantics.
