# Component: GlobalSearchDialog
- **Minimum Viable Component:** Global modal search surface for discovering sessions, pull requests, specs, tasks, roadmap items, actions, and projects from one query.
- **Props Interface:** `isOpen: boolean`; `query: string`; `activeDomainFilter: string | null`; `hasRecentItems: boolean`; `onQueryChange: (value) => void`; `onDomainFilterChange: (value) => void`; `onClose: () => void`.
- **Sub-components:** `RecentItemsList`, `GlobalSearchResults`.
- **State Complexity:** Simple UI-only dialog focus trap and highlighted-result state; search indexing and result ranking belong in `GlobalSearchState`.
- **Required Context:** `GlobalSearchContext`.
- **Tauri IPC:** None.
- **Interactions & Events:** Opens from a global shortcut; filters by domain; navigates results with keyboard input; opens a result in the correct page or detail tab.
