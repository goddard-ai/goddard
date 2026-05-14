# Component: CodeDiffView

- **Current Baseline:** Session changes currently render through `app/src/session-changes/view.tsrx` as a simple daemon-backed raw diff tab. That path should remain acceptable until a richer review surface is needed by more than one feature.
- **Minimum Viable Follow-On:** Reusable diff review view for one daemon-provided or locally computed diff source, starting from plain text diff rendering and file navigation before adding split panes or a third-party diff renderer.
- **Props Interface:** `source: { id, kind: "session" | "pullRequest" | "turn", title, projectPath?: string | null }`; `diff: { text: string, workspaceRoot?: string | null, files?: array of normalized file diff records }`; `selectedFilePath?: string | null`; `onSelectFile?: (path: string) => void`; `onRefresh?: () => void`.
- **Sub-components:** Optional file navigator and per-file diff sections only after the daemon or normalization layer can provide stable file boundaries.
- **State Complexity:** Keep local presentation state in the component while only one caller exists. Add shared diff state only when multiple tabs or embedded sections need the same loaded diff, selected file, and restoration behavior.
- **Required Context:** None for the first reusable pass. Use feature-local mutation/query helpers rather than a global context until there is real sharing pressure.
- **Electrobun RPC:** None directly; loading should route through SDK, daemon, or host adapters owned by the calling feature.
- **Interactions & Events:** Shows empty, unavailable, clean, and changed states; preserves tab scroll through the shell; optionally selects files; opens a refresh action when the source can be reloaded.
