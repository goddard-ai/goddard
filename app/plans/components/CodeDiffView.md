# Component: CodeDiffView

- **Minimum Viable Component:** Reusable wrapper around `@pierre/diffs/react` that renders one normalized diff source in a review-friendly layout.
- **Props Interface:** `diffSource: { id, label, kind, metadata }`; `files: array of diff file records`; `selectedFilePath?: string`; `presentationMode?: "unified" | "split"`; `onSelectFile?: (path) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple local file-pane scrolling and viewport restoration; diff loading and normalization belong in `CodeDiffState`.
- **Required Context:** `CodeDiffContext` when connected; otherwise none.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects files or hunks when a file navigator exists; changes presentation mode if supported; preserves scroll when reopened from tab cache.
