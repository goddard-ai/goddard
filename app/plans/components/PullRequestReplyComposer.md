# Component: PullRequestReplyComposer

- **Minimum Viable Follow-On:** Reply form inside a pull request detail tab for posting one managed response after auth, daemon reply submission, and protected-action behavior are available.
- **Props Interface:** `draft: string`; `canSubmit: boolean`; `isSubmitting: boolean`; `errorMessage?: string | null`; `placeholder?: string`; `onDraftChange: (value: string) => void`; `onSubmit: () => void`; `onCancel?: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only textarea sizing and keyboard shortcut state. Reply draft state belongs in `PullRequestComposeState` only when drafts must survive tab switches or multiple PR tabs.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Edits the reply body; submits one managed reply; preserves draft text on failure; cancels or clears the draft when desired.
