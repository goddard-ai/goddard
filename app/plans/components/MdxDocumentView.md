# Component: MdxDocumentView
- **Minimum Viable Component:** Document surface for viewing and editing MDX content with project breadcrumbs, a mode-aware toolbar, and an `@mdxeditor/editor` integration boundary.
- **Props Interface:** `documentId: string`; `source: { kind, pathOrKey }`; `initialMode?: "view" | "edit"`; `readOnly?: boolean`.
- **Sub-components:** `DocumentBreadcrumbs`, `MdxDocumentToolbar`, `MdxEditorSurface`.
- **State Complexity:** Simple local split-pane sizing only; document loading, dirty tracking, and save workflows belong in `MdxDocumentState`.
- **Required Context:** `MdxDocumentContext`, `ProjectContentContext`.
- **Electrobun RPC:** None directly; document I/O should route through state or service adapters.
- **Interactions & Events:** Loads a document by stable id; switches between view and edit modes; saves or reverts edits; can be opened from the main workbench or a detail tab.
