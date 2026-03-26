# Component: MdxDocumentToolbar
- **Minimum Viable Component:** Toolbar for MDX mode switching, save/revert actions, and document metadata display.
- **Props Interface:** `document: { title, pathOrKey, updatedAt }`; `mode: "view" | "edit"`; `isDirty: boolean`; `isSaving: boolean`; `readOnly: boolean`; `onModeChange: (mode) => void`; `onSave: () => void`; `onRevert: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only menu and disabled-state presentation.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Switches modes; saves pending edits; reverts to the last persisted revision.
