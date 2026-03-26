# Component: SessionChatHeader
- **Minimum Viable Component:** Summary header for a chat tab showing the session title, repository context, lifecycle status, and quick navigation actions.
- **Props Interface:** `session: { id, title, repositorySlug, rawStatus, displayStatusLabel, updatedAt, initiative, blockedReason }`; `connection: { mode, reconnectable }`; `onReconnect: () => void`; `onStop: () => void`; `onOpenDiff: () => void`; `onOpenPullRequest: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only button disabled states and overflow menu presentation.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Reconnects to history or live mode; stops the session; opens the latest diff or pull request in another tab.
