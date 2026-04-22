# Component: PullRequestList

- **Minimum Viable Component:** Recency-ordered list of pull requests with click-through into detail tabs and a compact review summary.
- **Props Interface:** `pullRequests: array of pull request summary records`; `selectedPullRequestRef?: { owner, repo, number } | null`; `onSelect: (ref) => void`; `onOpen: (ref) => void`.
- **Sub-components:** `PullRequestListRow`.
- **State Complexity:** Simple UI-only keyboard focus and list virtualization state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects one pull request; opens a pull request detail tab; preserves the current filter context while navigating.
