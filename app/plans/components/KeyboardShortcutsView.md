# Component: KeyboardShortcutsView

- **Minimum Viable Component:** Closable workbench-tab view for browsing every registered shortcut command, filtering the list, and opening a lightweight capture modal for rebinding.
- **Props Interface:** `class?: string`.
- **Sub-components:** `KeyboardShortcutCaptureDialog`.
- **State Complexity:** Simple local search text, recording-search toggle state, selected command id, and modal-open state; registered command data, recording ownership, and persistence belong in `ShortcutRegistry`.
- **Required Context:** `ShortcutRegistryContext`, `WorkbenchTabsContext` only if the view later links out to other workbench surfaces.
- **Electrobun RPC:** None directly; persistence and menu refresh should route through `ShortcutRegistry`.
- **Interactions & Events:** Filters rows by typed text; toggles recording-search mode from the keyboard-icon button; exits recording-search mode with `Escape`; filters by recorded shortcut expression; opens the capture modal when a row is clicked; saves or resets one command’s bindings through the registry.
