# State Module: PullRequestState

- **Responsibility:** Manage pull request detail records, collapsed discussion summaries, and on-demand expansion of the full discussion.
- **Data Shape:** One map keyed by stable pull request ref `{ owner, repo, number }`; summary metadata for title, author, branches, status, URL, and repository; discussion slices containing author description, hidden count, last non-author reply, last author reply, and optional full timeline; loading and error state.
- **Mutations/Actions:** `loadPullRequest`; `refreshPullRequest`; `revealFullDiscussion`; `mergePullRequestEvent`; `setActiveDiscussionMode`; `openRelatedSession`.
- **Scope & Hoisting:** Hoisted into a shared provider keyed by pull request ref so tabs can reuse fetched PR state and the inbox or session list can deep-link into it.
- **Side Effects:** Fetches pull request data from SDK-backed services or backend adapters; listens to relevant realtime events so open PR views stay current; coordinates with `CodeDiffState` to load the diff payload referenced by the pull request.
