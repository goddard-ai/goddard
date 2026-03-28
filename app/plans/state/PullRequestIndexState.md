# State Module: PullRequestIndexState
- **Responsibility:** Own the filtered index of pull requests shown in the primary pull requests page.
- **Data Shape:** Normalized pull request summaries keyed by stable pull request ref; ordered visible ids; filter fields for query, repository, state, author, and managed status; pagination metadata; loading and error state.
- **Mutations/Actions:** `loadPullRequests`; `loadMorePullRequests`; `refreshPullRequests`; `mergePullRequestSummary`; `setPullRequestFilters`; `openPullRequest`; `clearPullRequestFilters`.
- **Scope & Hoisting:** Hoisted into a shared provider because pull request summaries are reused by the index page, inbox deep links, global search, and detail-tab open flows.
- **Side Effects:** Fetches pull request summaries from shared SDK-backed services; merges realtime activity updates that affect indexed pull requests; routes open actions through `WorkbenchTabsState`.
