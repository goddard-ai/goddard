# Component: BrowserPreviewToolbar
- **Minimum Viable Component:** Browser-style control row containing back, forward, refresh, stop, and an editable URL field.
- **Props Interface:** `displayUrl: string`; `committedUrl: string | null`; `canGoBack: boolean`; `canGoForward: boolean`; `isLoading: boolean`; `onBack: () => void`; `onForward: () => void`; `onRefresh: () => void`; `onStop?: () => void`; `onUrlChange: (value) => void`; `onUrlCommit: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only input editing and focus state.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Navigates back or forward; refreshes the current page; commits a new URL; optionally stops an in-flight navigation.
