# Component: WorkbenchTab
- **Minimum Viable Component:** Individual tab chip that shows icon, title, optional dirty/status marker, and a close button that only appears on hover.
- **Props Interface:** `tab: { id, title, icon, closable, dirty?, statusBadge? }`; `isActive: boolean`; `isPrimary: boolean`; `isDragSource?: boolean`; `isDropTarget?: boolean`; `onSelect: (id) => void`; `onClose?: (id) => void`; `onDragStart?: (id) => void`; `onDragEnter?: (id) => void`; `onDragEnd?: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover, pressed, and drag handle presentation.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Click selects the tab; hover reveals the close button for closable tabs; middle-click or close-button click emits close; drag gestures emit reorder hooks.
