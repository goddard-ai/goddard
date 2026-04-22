# Component: PullRequestListRow

- **Minimum Viable Component:** One pull request row that shows repository, number, title, author, managed status, and updated time.
- **Props Interface:** `pullRequest: { owner, repo, number, title, author, status, managed, updatedAt }`; `isSelected: boolean`; `onSelect: (ref) => void`; `onOpen: (ref) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover and pressed state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects the row; opens the pull request detail tab; surfaces managed status as a stable visual cue.
