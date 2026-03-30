# Component: PullRequestHeader
- **Minimum Viable Component:** Summary banner for pull request identity, repository context, author, branch metadata, contextual actions, and high-level status links.
- **Props Interface:** `pullRequest: { owner, repo, number, title, author, createdAt, status, headBranch, baseBranch, url }`; `relatedSessionId?: string`; `currentTabContext?: { kind, projectPath?, entityRef? } | null`; `onOpenInBrowser: () => void`; `onOpenSession?: () => void`; `onRefresh: () => void`; `onActionSelect?: (actionId) => void`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple UI-only button and overflow presentation.
- **Required Context:** `ActionCatalogContext` when the contextual action menu is connected here.
- **Tauri IPC:** None.
- **Interactions & Events:** Opens the canonical PR URL; refreshes the current PR data; optionally jumps to the linked session chat tab; launches a contextual action from the current pull request tab.
