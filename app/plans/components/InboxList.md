# Component: InboxList
- **Minimum Viable Component:** Dense list container for inbox rows with multi-select support and hover-driven action reveal.
- **Props Interface:** `items: array of inbox item summaries`; `selectedIds: set-like collection of inbox ids`; `onToggleSelected: (id) => void`; `onOpen: (id) => void`; `onSnooze: (id) => void`; `onArchive: (id) => void`; `onDelegate: (id) => void`.
- **Sub-components:** `InboxRow`.
- **State Complexity:** Simple UI-only range selection and keyboard focus behavior.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects one or many rows; opens an inbox item’s linked work surface; exposes per-row snooze, archive, and delegate actions.
