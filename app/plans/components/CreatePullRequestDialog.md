# Component: CreatePullRequestDialog
- **Minimum Viable Component:** Modal flow for submitting one new managed pull request from project context and current session or diff context.
- **Props Interface:** `isOpen: boolean`; `draft: { projectPath, title, body, headBranch, baseBranch, sourceSessionId? }`; `projects: array of project summaries`; `validation: { canSubmit, fieldErrors }`; `isSubmitting: boolean`; `onDraftChange: (patch) => void`; `onSubmit: () => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only form layout and branch suggestion presentation; compose state belongs in `PullRequestComposeState`.
- **Required Context:** `PullRequestComposeContext`, `ProjectRegistryContext`.
- **Tauri IPC:** None directly; submission should route through SDK-backed compose state.
- **Interactions & Events:** Selects the source project and branches; edits title and body; submits the managed pull request; opens the resulting PR tab on success.
