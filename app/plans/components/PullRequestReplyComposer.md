# Component: PullRequestReplyComposer

- **Minimum Viable Component:** Reply form inside a pull request detail tab for posting one managed response back to the pull request.
- **Props Interface:** `draft: string`; `canSubmit: boolean`; `isSubmitting: boolean`; `placeholder?: string`; `onDraftChange: (value) => void`; `onSubmit: () => void`; `onCancel?: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only textarea sizing and keyboard shortcut state; reply draft state belongs in `PullRequestComposeState`.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Edits the reply body; submits one managed reply; cancels or clears the draft when desired.
