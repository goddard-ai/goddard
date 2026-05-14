# Component: PullRequestHeader

- **Current Baseline:** Pull request identity is currently rendered inline in `PullRequestView`.
- **Minimum Viable Follow-On:** Extract a header only when the detail tab gains enough actions or metadata to justify a child component.
- **Props Interface:** `pullRequest: DaemonPullRequest`; `projectPath: string | null`; `url: string`; `isRefreshing?: boolean`; `onRefresh?: () => void`; `onOpenRelatedSession?: () => void`; `onActionSelect?: (actionId: string) => void`.
- **Sub-components:** `ContextActionDropdown` only after actions are implemented for pull request context.
- **State Complexity:** Simple UI-only button and overflow presentation.
- **Required Context:** None for the first extraction.
- **Electrobun RPC:** None.
- **Interactions & Events:** Opens the canonical PR URL; refreshes the current PR data when supported; optionally jumps to a linked session; launches contextual actions later.
