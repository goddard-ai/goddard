# Component: RecentItemsList
- **Minimum Viable Component:** Compact list of recent pages and tabs shown inside the global search dialog before the user types or when the query is empty.
- **Props Interface:** `items: array of { id, domain, title, subtitle?, icon }`; `onOpen: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover and keyboard focus state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Opens one recent item; yields to ranked search results once the query becomes non-empty.
