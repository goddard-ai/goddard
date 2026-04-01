# Component: PullRequestsPage
- **Minimum Viable Component:** Full-width pull request index page for triaging managed pull requests, filtering them, and opening detail tabs.
- **Props Interface:** `class?: string`; `embedded?: boolean`.
- **Sub-components:** `PullRequestFilterSidebar`, `PullRequestList`, `CreatePullRequestDialog`.
- **State Complexity:** Simple local empty-state and sidebar sizing; index data belongs in `PullRequestIndexState`, and compose workflows belong in `PullRequestComposeState`.
- **Required Context:** `PullRequestIndexContext`, `PullRequestComposeContext`, `WorkbenchTabsContext`.
- **Electrobun RPC:** None directly; PR list and compose actions should route through SDK-backed state modules.
- **Interactions & Events:** Filters pull requests; opens an existing PR detail tab; creates a new PR; refreshes the current list.
