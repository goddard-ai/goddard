# Component: TerminalViewport
- **Minimum Viable Component:** `xterm.js` host element that renders terminal bytes, captures keyboard and paste input, and emits fit-calculated dimensions upward.
- **Props Interface:** `sessionId: string`; `theme?: { background, foreground, cursor }`; `font?: { family, size, lineHeight }`; `onReady: () => void`; `onResize: (cols, rows) => void`; `onInput: (data) => void`; `onPaste?: (data) => void`; `onFocus?: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only terminal instance lifecycle and resize observation; PTY ownership and resize side effects belong in `TerminalSessionState`.
- **Required Context:** None.
- **Electrobun RPC:** None directly; it emits dimensions and input events to state, which must forward them through the Electrobun bridge so the PTY host can resize correctly.
- **Interactions & Events:** Mounts the terminal instance; forwards keyboard and paste input; uses the fit addon to calculate rows and columns; emits resize events whenever the viewport changes size.
