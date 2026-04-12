# Component: MarkdownDocumentToolbar
- **Minimum Viable Component:** Toolbar for markdown document mode switching, save or revert actions, contextual actions, and document metadata display.
- **Props Interface:** `document: { title, pathOrKey, updatedAt }`; `mode: "view" | "edit"`; `isDirty: boolean`; `isSaving: boolean`; `readOnly: boolean`; `currentTabContext?: { kind, projectPath?, entityRef? } | null`; `onModeChange: (mode) => void`; `onSave: () => void`; `onRevert: () => void`; `onActionSelect?: (actionId) => void`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple UI-only menu and disabled-state presentation.
- **Required Context:** `ActionCatalogContext` when the contextual action menu is connected here.
- **Electrobun RPC:** None.
- **Interactions & Events:** Switches modes; saves pending edits; reverts to the last persisted revision; launches a contextual action from the current document tab.
