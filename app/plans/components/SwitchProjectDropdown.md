# Component: SwitchProjectDropdown
- **Minimum Viable Component:** Header-anchored dropdown for viewing the active project, searching opened projects, switching active project context, and dispatching `Open folder…` from an empty query.
- **Props Interface:** `isOpen: boolean`; `activeProjectPath: string | null`; `projects: readonly { path, name }[]`; `recentProjectPaths: readonly string[]`; `onOpenChange: (isOpen) => void`; `onSelectProject: (path) => void`; `onOpenFolder: () => Promise<void> | void`.
- **Sub-components:** Search input; project result list; empty-state row; optional active-project indicator row styling.
- **State Complexity:** Local UI-only search query and highlighted-result state. Persisted active-project semantics and recency ordering belong in `ProjectContextState`.
- **Required Context:** None required if the parent passes normalized project rows and action handlers.
- **Electrobun RPC:** Indirect only through the existing `Open folder` action.
- **Interactions & Events:** Opens from the header button or `Mod+o`; auto-focuses search; filters by project name and path; highlights the first row by default; selects highlighted row on `Enter`; opens or focuses the chosen project tab; hides `Open folder…` once the query is non-empty.
