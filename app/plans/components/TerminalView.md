# Component: TerminalView
- **Minimum Viable Component:** Detail view for one PTY-backed terminal session that composes session controls and an `xterm.js` viewport.
- **Props Interface:** `terminalSessionId: string`; `initialCwdLabel?: string`; `initialTitle?: string`.
- **Sub-components:** `TerminalToolbar`, `TerminalViewport`.
- **State Complexity:** Simple local pane sizing only; PTY lifecycle, byte streaming, resize coordination, and restart behavior belong in `TerminalSessionState`.
- **Required Context:** `TerminalSessionContext`.
- **Tauri IPC:** None directly; terminal IPC should be owned by `TerminalSessionState`.
- **Interactions & Events:** Starts or reconnects a terminal session; forwards toolbar actions; keeps the viewport bound to a live PTY while the tab remains cached.
