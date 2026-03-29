# Component: SessionChatView
- **Minimum Viable Component:** Detail view for one coding agent session that composes the header, transcript, and composer around an `@assistant-ui/react` integration boundary.
- **Props Interface:** `sessionId: string`; `tabId: string`; `initialFocus?: "transcript" | "composer"`.
- **Sub-components:** `SessionChatHeader`, `SessionChatTranscript`, `SessionChatComposer`.
- **State Complexity:** Simple local scroll anchoring only; transcript loading, live connection, message sending, and draft persistence belong in `SessionChatState`.
- **Required Context:** `SessionChatContext`.
- **Tauri IPC:** None directly; all daemon or backend communication should be triggered via state actions.
- **Interactions & Events:** Loads the requested session conversation; streams incoming updates; sends prompts; can stop, reconnect, or open related review tabs from the header.
