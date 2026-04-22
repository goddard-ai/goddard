# Component: TerminalToolbar

- **Minimum Viable Component:** Toolbar showing terminal title, cwd, connection state, contextual actions, and session-level controls such as restart and clear.
- **Props Interface:** `terminal: { title, cwdLabel, connectionState, exitCode? }`; `currentTabContext?: { kind, projectPath?, entityRef? } | null`; `canRestart: boolean`; `onRestart: () => void`; `onClear: () => void`; `onClose: () => void`; `onActionSelect?: (actionId) => void`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple UI-only disabled-state and overflow handling.
- **Required Context:** `ActionCatalogContext` when the contextual action menu is connected here.
- **Electrobun RPC:** None.
- **Interactions & Events:** Restarts a stopped PTY; clears visible scrollback; closes the tab; reflects connection or exit changes; launches a contextual action from the current terminal tab.
