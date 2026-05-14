# Component: PullRequestView

- **Current Baseline:** `app/src/pull-requests/view.tsrx` opens a daemon-managed pull request by `pullRequestId`, renders repository identity, basic metadata, project affinity, and a canonical GitHub link.
- **Minimum Viable Follow-On:** Extend the existing detail tab only as daemon PR data becomes richer. Keep the first detail surface focused on identity, status, linked workspace, external navigation, and recoverable loading errors.
- **Props Interface:** `pullRequestId: DaemonPullRequestId`; `projectPath: string | null`.
- **Sub-components:** `PullRequestHeader` can replace the current inline header when it needs more actions. `PullRequestDiscussionSummary`, `PullRequestReplyComposer`, and embedded `CodeDiffView` are follow-ons gated on daemon discussion, reply, and diff contracts.
- **State Complexity:** Keep basic loading in `useQuery(goddardSdk.pr.get)`. Add `PullRequestState` only when detail tabs need discussion expansion, realtime merging, or shared data beyond query cache behavior.
- **Required Context:** None for the current detail tab beyond existing app state hooks. Add compose or diff providers only when those features land.
- **Electrobun RPC:** None directly; external navigation should stay a normal link or route through a host helper only when the app needs native open behavior.
- **Interactions & Events:** Retry failed loads; open the canonical PR URL; report the tab's project path; later refresh discussion, open related sessions, compose replies, and inspect diffs.
