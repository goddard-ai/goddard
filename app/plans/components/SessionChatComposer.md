# Component: SessionChatComposer
- **Minimum Viable Component:** Prompt composer and send controls for continuing a session conversation.
- **Props Interface:** `draft: string`; `canSend: boolean`; `isSending: boolean`; `placeholder?: string`; `onDraftChange: (value) => void`; `onSend: () => void`; `onCancel?: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only textarea sizing and keyboard shortcut handling.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Typing updates the draft; Enter or button click sends the prompt; modifier-aware shortcuts preserve multiline editing; optional cancel stops an in-flight send.
