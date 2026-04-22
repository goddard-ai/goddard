# Component: SessionChatHeader

- **Minimum Viable Component:** Summary header for a chat tab showing the session title, project context, lifecycle status, contextual actions, and quick navigation actions.
- **Props Interface:** `session: { id, title, repositorySlug, rawStatus, displayStatusLabel, updatedAt, initiative, blockedReason }`; `connection: { mode, reconnectable }`; `currentTabContext?: { kind, projectPath?, entityRef? } | null`; `onReconnect: () => void`; `onStop: () => void`; `onOpenDiff: () => void`; `onOpenPullRequest: () => void`; `onActionSelect?: (actionId) => void`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple UI-only button disabled states and overflow menu presentation.
- **Required Context:** `ActionCatalogContext` when the contextual action menu is connected here.
- **Electrobun RPC:** None.
- **Interactions & Events:** Reconnects to history or live mode; stops the session; opens the latest diff or pull request in another tab; launches a contextual action from the current session tab.
