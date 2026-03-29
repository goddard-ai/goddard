# Component: SidebarNavItem
- **Minimum Viable Component:** Single icon button inside the sidebar that renders selected, unselected, disabled, and badge-bearing states accessibly.
- **Props Interface:** `item: { id, icon, label, ariaLabel, badgeCount?, disabled? }`; `isSelected: boolean`; `isFocused?: boolean`; `onSelect: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only focus ring and hover presentation.
- **Required Context:** None.
- **Tauri IPC:** None.
- **Interactions & Events:** Hover shows tooltip intent; click or keyboard activation emits `onSelect(item.id)`.
