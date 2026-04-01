# Component: GlobalSearchResults
- **Minimum Viable Component:** Ranked result list for the global search dialog that groups hits by domain and supports keyboard-first navigation.
- **Props Interface:** `results: array of ranked result records`; `highlightedResultId?: string | null`; `onHighlight: (id) => void`; `onOpen: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only highlighted-row and scroll-into-view state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Highlights one result; opens it in the appropriate page or tab; reflects search ranking updates as the query changes.
