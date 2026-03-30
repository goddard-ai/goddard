# Component: SessionLaunchForm
- **Minimum Viable Component:** Form body inside the launch dialog for selecting project scope, action preset, optional runtime flags, and the initial prompt text.
- **Props Interface:** `draft: { projectPath, actionId, initialPrompt, worktreeEnabled, cwdMode }`; `projects: array of project summaries`; `availableActions: array of action summaries`; `validation: { canSubmit, fieldErrors }`; `isSubmitting: boolean`; `onDraftChange: (patch) => void`; `onSubmit: () => void`; `onCancel: () => void`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple UI-only form focus and disclosure state; draft and validation belong in `SessionLaunchState`.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Selects project scope; filters and chooses an action; edits the initial prompt; toggles worktree or cwd settings; submits or cancels the launch.
