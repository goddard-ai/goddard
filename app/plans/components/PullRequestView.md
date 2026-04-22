# Component: PullRequestView

- **Minimum Viable Component:** GitHub-like pull request detail view that emphasizes the author description, a collapsed discussion summary, a reply flow, and the code diff at the bottom.
- **Props Interface:** `pullRequestRef: { owner, repo, number }`; `sourceSessionId?: string`; `initialDiscussionMode?: "collapsed" | "expanded"`.
- **Sub-components:** `PullRequestHeader`, `PullRequestDiscussionSummary`, `PullRequestReplyComposer`, `CodeDiffView`.
- **State Complexity:** Simple local anchor-link and section-collapse presentation; pull request data loading and discussion expansion belong in `PullRequestState`, while reply drafting belongs in `PullRequestComposeState`.
- **Required Context:** `PullRequestContext`, `PullRequestComposeContext`, `CodeDiffContext`.
- **Electrobun RPC:** None directly; external navigation or refresh actions should route through service or state layers.
- **Interactions & Events:** Loads a PR by stable reference; reveals the full discussion when requested; drafts and submits one managed reply; scrolls to the embedded diff; can open related repository URLs through header callbacks.
