# Component: SessionLaunchForm

- **Minimum Viable Component:** Form body inside the launch dialog for selecting project scope, action preset, optional runtime flags, and the initial prompt text.
- **Props Interface:** `draft: { projectPath, actionId, initialPrompt, worktreeEnabled, cwdMode }`; `projects: array of project summaries`; `availableActions: array of action summaries`; `validation: { canSubmit, fieldErrors }`; `isSubmitting: boolean`; `onDraftChange: (patch) => void`; `onSubmit: () => void`; `onCancel: () => void`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Pure form rendering plus lightweight focus and disclosure state; the parent dialog owns the editable draft, validation, and submit lifecycle.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects project scope; filters and chooses an action; edits the initial prompt; toggles worktree or cwd settings; submits or cancels the launch.
