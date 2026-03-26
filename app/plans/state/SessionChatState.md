# State Module: SessionChatState
- **Responsibility:** Manage one or many live or history-backed chat conversations for daemon-managed coding sessions, isolated from the `@assistant-ui/react` view layer.
- **Data Shape:** One map keyed by session id containing summary metadata, connection mode, reconnectability, normalized message history, pending draft text, send status, unread markers, streaming markers, and related diff or pull request references.
- **Mutations/Actions:** `connectSession`; `loadSessionHistory`; `appendIncomingMessage`; `updateDraft`; `sendPrompt`; `cancelPrompt`; `reconnectSession`; `markTranscriptViewed`; `disposeSessionConnection`.
- **Scope & Hoisting:** Hoisted into a shared provider keyed by session id so multiple tabs can reconnect or restore a conversation without duplicating daemon connections.
- **Side Effects:** Uses the daemon-backed SDK session contract to connect, fetch history, send prompts, and receive streamed messages; keeps live subscriptions open only while the session has active consumers or cached tabs that require quick restoration.
