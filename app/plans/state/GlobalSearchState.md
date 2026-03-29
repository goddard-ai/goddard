# State Module: GlobalSearchState
- **Responsibility:** Own the global discovery index, recent items, current query, and ranked search results across app domains.
- **Data Shape:** Dialog open state; current query; active domain filter; ranked result list; highlighted result id; recent item list; indexing status; last refresh metadata.
- **Mutations/Actions:** `openSearchDialog`; `closeSearchDialog`; `setSearchQuery`; `setSearchDomainFilter`; `rebuildSearchIndex`; `highlightResult`; `openSearchResult`; `recordRecentItem`.
- **Scope & Hoisting:** Hoisted into a global provider because search needs read access across sessions, pull requests, specs, tasks, roadmap items, actions, and repositories.
- **Side Effects:** Builds a lightweight cross-domain index from existing state modules; may use client-side fuzzy matching over normalized records; routes selected results to the correct main page or detail tab.
