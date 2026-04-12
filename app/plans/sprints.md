# Sprint Plan

- Scope:
  - This file prioritizes the currently planned, not-yet-implemented app components and state modules into implementation sprints.
  - The sequence is dependency-first and MVP-first.
  - All remaining planned components and state modules are assigned below, including explicitly blocked work.

- Sequencing rules:
  - Assume the implemented shell, project registry, and tab persistence remain the foundation for later feature pages.
  - Delay auth and auth-dependent workflows as long as possible so early implementation stays local-first.
  - Build domain state before connected page components in the same sprint.
  - Land cross-domain features such as search only after enough domains exist to index meaningfully.
  - Keep explicitly deferred or spec-blocked work out of the active MVP sequence.

## Sprint 1: Sessions and Session Launch

- Goal:
  - Make session steering real: browse sessions, launch new ones through a modal-local form flow, and work inside session chat tabs.
- State:
  - `SessionIndexState`
  - `SessionChatState`
  - Optional thin `SessionLaunchState` only if multiple surfaces need one shared dialog launcher; keep launch draft and validation local to `NewSessionDialog`.
- Components:
  - `SessionsPage`
  - `SessionListToolbar`
  - `SessionList`
  - `SessionListRow`
  - `NewSessionDialog`
  - `SessionLaunchForm`
  - `SessionChatView`
  - `SessionChatHeader`
  - `SessionChatTranscript`
  - `SessionChatComposer`
- Why here:
  - Sessions are the center of the product loop, and several later features open or derive from session context.

## Sprint 2: Actions and Contextual Action Entry Points

- Goal:
  - Make actions a first-class concept for creation, editing, filtering, and contextual launch.
- State:
  - `ActionCatalogState`
  - `ActionDraftState`
- Components:
  - `ActionsPage`
  - `ActionFilterSidebar`
  - `ActionList`
  - `ActionListRow`
  - `ActionEditorView`
  - `CreateActionDialog`
  - `ContextActionDropdown`
- Why here:
  - Session launch, many tab headers, and later operator workflows depend on reusable action definitions and contextual action resolution.

## Sprint 3: Local Diff Review

- Goal:
  - Add local diff review first, without pulling auth-dependent remote review flows forward.
- State:
  - `CodeDiffState`
- Components:
  - `CodeDiffView`
- Why here:
  - Diff viewing supports local-first review workflows and is reusable later inside pull request tabs.

## Sprint 4: Specs and Markdown Document Workflows

- Goal:
  - Add project-scoped document discovery and editable markdown document tabs for specification management.
- State:
  - `ProjectContentState`
  - `MarkdownDocumentState`
- Components:
  - `SpecsPage`
  - `SpecTreeSidebar`
  - `SpecFileList`
  - `DocumentBreadcrumbs`
  - `MarkdownDocumentView`
  - `MarkdownDocumentToolbar`
  - `MarkdownEditorSurface`
- Why here:
  - Specification management depends on the project registry and tab system, but does not need pull request or roadmap work to exist first.

## Sprint 5: Tasks and Roadmap Lists

- Goal:
  - Add the list-first planning surfaces for near-term tasks and longer-term roadmap proposals.
- State:
  - `TaskListState`
  - `RoadmapState`
- Components:
  - `TasksPage`
  - `TaskFilterSidebar`
  - `TaskList`
  - `TaskListRow`
  - `TaskDetailView`
  - `RoadmapPage`
  - `ProposalFilterSidebar`
  - `ProposalList`
  - `ProposalListRow`
  - `ProposalDetailView`
- Why here:
  - These pages reuse the list, tab, project, and action patterns established earlier, but they are not prerequisites for the core session and pull request loop.

## Sprint 6: Global Search and Loop Operations

- Goal:
  - Add cross-domain discovery and the loop runtime operator page once enough domain data exists to make them useful.
- State:
  - `GlobalSearchState`
  - `LoopRuntimeState`
- Components:
  - `GlobalSearchDialog`
  - `GlobalSearchResults`
  - `RecentItemsList`
  - `LoopsPage`
  - `LoopFilterSidebar`
  - `LoopList`
  - `LoopListRow`
  - `StartLoopDialog`
- Why here:
  - Search quality improves after most domains exist, and loop control is valuable once the app already has projects, sessions, and actions in place.

## Sprint 7: Auth, Pull Requests, Inbox, and Realtime Review

- Goal:
  - Add lazy auth as late as possible, then layer on the remote-backed review surfaces that depend on authenticated identity.
- State:
  - `AuthState`
  - `PullRequestIndexState`
  - `PullRequestState`
  - `PullRequestComposeState`
  - `InboxState`
  - `RealtimeActivityState`
- Components:
  - `IdentityPage`
  - `ProtectedActionGate`
  - `DeviceFlowDialog`
  - `PullRequestsPage`
  - `PullRequestFilterSidebar`
  - `PullRequestList`
  - `PullRequestListRow`
  - `CreatePullRequestDialog`
  - `PullRequestView`
  - `PullRequestHeader`
  - `PullRequestDiscussionSummary`
  - `PullRequestReplyComposer`
  - `InboxPage`
  - `InboxToolbar`
  - `InboxList`
  - `InboxRow`
- Why here:
  - This keeps early sprints local-first while still preserving the planned lazy-auth model and the managed pull request workflows that sit on top of it.

## Blocked: Runtime Surfaces Requiring Architecture Alignment

- Goal:
  - Keep explicitly planned runtime surfaces visible without treating them as active MVP work before the host-boundary question is resolved.
- State:
  - `TerminalSessionState`
  - `BrowserPreviewState`
- Components:
  - `TerminalView`
  - `TerminalToolbar`
  - `TerminalViewport`
  - `BrowserPreviewView`
  - `BrowserPreviewToolbar`
  - `BrowserPreviewFrame`
  - `BrowserPreviewConsole`
- Why blocked:
  - These plans currently assume capabilities that still need alignment with the app architecture and spec before implementation should begin.
