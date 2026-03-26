# Component: InboxRow
- **Minimum Viable Component:** One inbox notification row showing project name, notification title, relative updated time, and hover-only action buttons.
- **Props Interface:** `item: { id, projectName, title, updatedAt, state, linkedSessionId?, linkedPullRequestRef?, linkedDiffRef? }`; `isSelected: boolean`; `onToggleSelected: (id) => void`; `onOpen: (id) => void`; `onSnooze: (id) => void`; `onArchive: (id) => void`; `onDelegate: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover state for action visibility.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Checkbox toggles selection; clicking the row opens the linked work; hovering reveals snooze, archive, and delegate buttons; action clicks do not trigger row-open by default.
