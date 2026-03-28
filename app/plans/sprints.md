# Sprint Plan

- Scope:
  - This file prioritizes the currently planned app components and state modules into implementation sprints.
  - The sequence is dependency-first and MVP-first.
  - All currently planned components and state modules are assigned below, including explicitly blocked work.

- Sequencing rules:
  - Build shell, repository registry, and tab persistence before feature pages.
  - Delay auth and auth-dependent workflows as long as possible so early implementation stays local-first.
  - Build domain state before connected page components in the same sprint.
  - Land cross-domain features such as search only after enough domains exist to index meaningfully.
  - Keep explicitly deferred or spec-blocked work out of the active MVP sequence.

## Sprint 1: Shell and Repository Registry

- Goal:
  - Make the app boot into a stable tab-first shell and establish the machine-wide repository registry.
- State:
  - `NavigationState`
  - `WorkbenchTabsState`
  - `RepositoryRegistryState`
- Components:
  - `AppShell`
  - `SidebarNav`
  - `SidebarNavItem`
  - `MainWorkbenchView`
  - `WorkbenchTabs`
  - `WorkbenchTab`
  - `WorkbenchTabPanel`
  - `RepositoriesPage`
  - `RepositoryList`
  - `RepositoryListRow`
  - `AddRepositoryDialog`
- Why here:
  - Every later feature depends on app-wide navigation, persistent tabs, and an explicit repository working set.

## Sprint 2: Sessions and Session Launch

- Goal:
  - Make session steering real: browse sessions, launch new ones through the modal flow, and work inside session chat tabs.
- State:
  - `SessionIndexState`
  - `SessionLaunchState`
  - `SessionChatState`
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

## Sprint 3: Actions and Contextual Action Entry Points

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

## Sprint 4: Local Diff Review

- Goal:
  - Add local diff review first, without pulling auth-dependent remote review flows forward.
- State:
  - `CodeDiffState`
- Components:
  - `CodeDiffView`
- Why here:
  - Diff viewing supports local-first review workflows and is reusable later inside pull request tabs.

## Sprint 5: Specs and MDX Document Workflows

- Goal:
  - Add repository-scoped document discovery and editable MDX document tabs for specification management.
- State:
  - `RepositoryContentState`
  - `MdxDocumentState`
- Components:
  - `SpecsPage`
  - `SpecTreeSidebar`
  - `SpecFileList`
  - `DocumentBreadcrumbs`
  - `MdxDocumentView`
  - `MdxDocumentToolbar`
  - `MdxEditorSurface`
- Why here:
  - Specification management depends on the repository registry and tab system, but does not need pull request or roadmap work to exist first.

## Sprint 6: Tasks and Roadmap Lists

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
  - These pages reuse the list, tab, repository, and action patterns established earlier, but they are not prerequisites for the core session and pull request loop.

## Sprint 7: Global Search and Loop Operations

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
  - Search quality improves after most domains exist, and loop control is valuable once the app already has repositories, sessions, and actions in place.

## Sprint 8: Auth, Pull Requests, Inbox, and Realtime Review

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
