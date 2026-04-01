# Component: MdxEditorSurface
- **Minimum Viable Component:** Wrapper around `@mdxeditor/editor` that adapts normalized MDX document state to a Preact-compatible editor surface.
- **Props Interface:** `document: { id, title, rawMdx, version }`; `mode: "view" | "edit"`; `readOnly: boolean`; `onChange: (nextMdx) => void`; `onSelectionChange?: (selection) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only editor instance lifecycle; content state and persistence live in `MdxDocumentState`.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Edits MDX in place; surfaces change events; reflects external document updates without losing explicit conflict markers.
