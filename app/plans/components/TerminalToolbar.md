# Component: TerminalToolbar
- **Minimum Viable Component:** Toolbar showing terminal title, cwd, connection state, and session-level controls such as restart and clear.
- **Props Interface:** `terminal: { title, cwdLabel, connectionState, exitCode? }`; `canRestart: boolean`; `onRestart: () => void`; `onClear: () => void`; `onClose: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only disabled-state and overflow handling.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Restarts a stopped PTY; clears visible scrollback; closes the tab; reflects connection or exit changes.
