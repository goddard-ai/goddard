# Component: SidebarNav
- **Minimum Viable Component:** Icon-only vertical navigation rail, similar to VSCode, that switches the primary workbench view and shows domain badge counts.
- **Props Interface:** `items: array of { id, icon, label, ariaLabel, badgeCount?, disabled? }`; `selectedItemId: string`; `onSelect: (id) => void`.
- **Sub-components:** `SidebarNavItem`.
- **State Complexity:** Simple UI-only state for keyboard focus and hover treatment.
- **Required Context:** `NavigationContext` if used as a connected container; otherwise none when driven entirely by props.
- **Tauri IPC:** None.
- **Interactions & Events:** Mouse click selects a domain; keyboard arrows move focus; Enter or Space activates an item; hover can reveal tooltip text.
