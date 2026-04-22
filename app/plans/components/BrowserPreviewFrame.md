# Component: BrowserPreviewFrame

- **Minimum Viable Component:** Sandboxed iframe host that renders the preview URL and forwards console-shim `postMessage` traffic back into preview state.
- **Props Interface:** `src: string`; `title?: string`; `loadingState: "idle" | "loading" | "ready" | "error"`; `onLoadStart?: () => void`; `onLoadCommit?: (url) => void`; `onLoadError?: (message) => void`; `onPostMessage: (payload) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only iframe lifecycle and window message subscription.
- **Required Context:** None.
- **Electrobun RPC:** None directly; the Bun-hosted preview protocol and header rewriting happen outside the component boundary.
- **Interactions & Events:** Loads the current preview URL; listens for injected console telemetry; emits committed navigation and error events.
