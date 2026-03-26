# Component: SessionChatTranscript
- **Minimum Viable Component:** Scrollable conversation timeline that adapts `@assistant-ui/react` primitives to the session history model and live streaming updates.
- **Props Interface:** `messages: array of normalized chat messages`; `isStreaming: boolean`; `hasOlderMessages: boolean`; `onLoadOlder: () => void`; `onRetryRender?: (messageId) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only scroll pinning and viewport restoration.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Scrolls through history; auto-pins to the newest message while the user remains at the bottom; requests older history when needed.
