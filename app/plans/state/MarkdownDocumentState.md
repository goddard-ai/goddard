# State Module: MarkdownDocumentState
- **Responsibility:** Own markdown document loading, editing, dirty tracking, and save or revert flows independently of the `MarkdownEditorSurface` component tree.
- **Data Shape:** One map keyed by document id containing source metadata, raw markdown content, editor-projected content, current mode, read-only flag, dirty baseline, save status, validation errors, and last-saved timestamp.
- **Mutations/Actions:** `loadDocument`; `setMode`; `editDocument`; `saveDocument`; `revertDocument`; `markExternalUpdate`; `clearDocumentError`.
- **Scope & Hoisting:** Hoisted into a shared provider keyed by document id so reopened tabs can restore editor state and dirty markers without re-fetching immediately.
- **Side Effects:** Reads and writes markdown document content through an injected document adapter; may persist unsaved drafts for crash recovery; should remain aligned with `core/sdk` ownership before any user-facing editing workflow ships because `AGENTS.md` requires app and SDK parity for new capabilities.
