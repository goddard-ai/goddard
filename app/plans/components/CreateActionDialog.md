# Component: CreateActionDialog

- **Minimum Viable Component:** Modal that captures the initial metadata required to create a new action before opening its editor tab.
- **Props Interface:** `isOpen: boolean`; `draft: { name, scope, projectPath }`; `projects: array of managed project summaries`; `validation: { canSubmit, fieldErrors }`; `isSubmitting: boolean`; `onDraftChange: (patch) => void`; `onSubmit: () => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only focus trap and validation message display; draft creation belongs in `ActionDraftState`.
- **Required Context:** `ActionDraftContext`, `ProjectRegistryContext`.
- **Electrobun RPC:** None.
- **Interactions & Events:** Captures a new action’s name and scope; creates the draft; closes or advances into the editor tab.
