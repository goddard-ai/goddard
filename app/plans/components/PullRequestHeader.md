# Component: PullRequestHeader
- **Minimum Viable Component:** Summary banner for pull request identity, repository context, author, branch metadata, and high-level status links.
- **Props Interface:** `pullRequest: { owner, repo, number, title, author, createdAt, status, headBranch, baseBranch, url }`; `relatedSessionId?: string`; `onOpenInBrowser: () => void`; `onOpenSession?: () => void`; `onRefresh: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only button and overflow presentation.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Opens the canonical PR URL; refreshes the current PR data; optionally jumps to the linked session chat tab.
