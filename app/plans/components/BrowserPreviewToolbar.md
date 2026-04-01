# Component: BrowserPreviewToolbar
- **Minimum Viable Component:** Browser-style control row containing navigation controls, an editable URL field, and contextual actions relevant to the current preview tab.
- **Props Interface:** `displayUrl: string`; `committedUrl: string | null`; `canGoBack: boolean`; `canGoForward: boolean`; `isLoading: boolean`; `currentTabContext?: { kind, projectPath?, entityRef? } | null`; `onBack: () => void`; `onForward: () => void`; `onRefresh: () => void`; `onStop?: () => void`; `onUrlChange: (value) => void`; `onUrlCommit: () => void`; `onActionSelect?: (actionId) => void`.
- **Sub-components:** `ContextActionDropdown`.
- **State Complexity:** Simple UI-only input editing and focus state.
- **Required Context:** `ActionCatalogContext` when the contextual action menu is connected here.
- **Electrobun RPC:** None.
- **Interactions & Events:** Navigates back or forward; refreshes the current page; commits a new URL; optionally stops an in-flight navigation; launches a contextual action from the current preview tab.
