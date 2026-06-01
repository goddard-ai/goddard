# State Module: PullRequestState

- **Current Baseline:** Basic pull request detail does not need a dedicated Sigma owner; `useQuery(goddardSdk.pr.get)` is sufficient for the current daemon-managed PR record.
- **Responsibility:** If introduced, manage richer pull request detail records, discussion expansion, related-session links, realtime merges, and refresh behavior that exceed query-cache ownership.
- **Data Shape:** Map keyed by `PullRequestId` containing the daemon PR record, optional discussion summary, optional full timeline, related session ids, load and refresh status, and last merged event timestamp.
- **Mutations/Actions:** `loadPullRequest`; `refreshPullRequest`; `mergePullRequestEvent`; `revealFullDiscussion`; `setDiscussionMode`; `openRelatedSession`; `clearPullRequestError`.
- **Scope & Hoisting:** Hoist only when PR detail data is reused across inbox links, search results, index rows, and open detail tabs. Keep single-tab detail reads query-driven until then.
- **Side Effects:** Fetch through SDK-backed PR APIs; invalidate affected query cache entries when simple refresh is enough; coordinate with `CodeDiffState` only after PR diff payloads exist.
