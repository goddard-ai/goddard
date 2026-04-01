# Component: ActionList
- **Minimum Viable Component:** Filtered list of available actions with create, edit, duplicate, and launch-entry affordances.
- **Props Interface:** `actions: array of action summary records`; `selectedActionId?: string | null`; `onSelect: (id) => void`; `onOpenEditor: (id) => void`; `onLaunchWithAction: (id) => void`.
- **Sub-components:** `ActionListRow`.
- **State Complexity:** Simple UI-only keyboard focus and empty-state handling.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects actions; opens an editor tab; starts a session launch flow prefilled with one action.
