# Component: BrowserPreviewView

- **Minimum Viable Component:** Detail view for an iframe-driven browser preview with navigation chrome and an observable console pane.
- **Props Interface:** `previewId: string`; `initialUrl?: string`; `showConsoleByDefault?: boolean`.
- **Sub-components:** `BrowserPreviewToolbar`, `BrowserPreviewFrame`, `BrowserPreviewConsole`.
- **State Complexity:** Simple local splitter sizing only; navigation history, loading state, and console capture belong in `BrowserPreviewState`.
- **Required Context:** `BrowserPreviewContext`.
- **Electrobun RPC:** None directly; preview navigation and protocol integration should be mediated by `BrowserPreviewState`.
- **Interactions & Events:** Navigates to URLs; toggles or resizes the console pane; refreshes and traverses history; keeps preview state alive while the tab remains cached.
