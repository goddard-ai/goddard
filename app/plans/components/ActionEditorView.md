# Component: ActionEditorView
- **Minimum Viable Component:** Detail-tab editor for one action definition, including metadata, prompt content, scope, and save workflow.
- **Props Interface:** `actionId: string`; `mode?: "edit" | "create"`; `sourceContext?: { repositoryId?: string | null }`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple local editor layout state; the actual draft, validation, and persistence workflow belong in `ActionDraftState`.
- **Required Context:** `ActionDraftContext`, `ActionCatalogContext`.
- **Tauri IPC:** None directly; action reads and writes should route through config and action state modules.
- **Interactions & Events:** Loads one action draft; edits metadata and prompt text; saves or reverts; can launch a new session with the in-progress action.
