# Component: CreatePullRequestDialog
- **Minimum Viable Component:** Modal flow for submitting one new managed pull request from repository context and current session or diff context.
- **Props Interface:** `isOpen: boolean`; `draft: { repositoryId, title, body, headBranch, baseBranch, sourceSessionId? }`; `repositories: array of managed repository summaries`; `validation: { canSubmit, fieldErrors }`; `isSubmitting: boolean`; `onDraftChange: (patch) => void`; `onSubmit: () => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only form layout and branch suggestion presentation; compose state belongs in `PullRequestComposeState`.
- **Required Context:** `PullRequestComposeContext`, `RepositoryRegistryContext`.
- **Tauri IPC:** None directly; submission should route through SDK-backed compose state.
- **Interactions & Events:** Selects repository and branches; edits title and body; submits the managed pull request; opens the resulting PR tab on success.
