# Component: SessionList
- **Minimum Viable Component:** Virtualizable list container for session rows with multi-select behavior and stable recency ordering.
- **Props Interface:** `sessions: array of session summary records`; `selectedIds: set-like collection of session ids`; `onToggleSelected: (id) => void`; `onOpenChat: (id) => void`; `onOpenDiff: (id) => void`; `onOpenPullRequest: (id) => void`.
- **Sub-components:** `SessionListRow`.
- **State Complexity:** Simple UI-only range selection and keyboard focus management.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Scrolls through sessions; toggles multi-select checkboxes; opens chat on row click; triggers diff or PR actions from row-level controls.
