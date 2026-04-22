# Component: ActionListRow

- **Minimum Viable Component:** One row in the action catalog showing the action name, scope, project applicability, and quick launch or edit actions.
- **Props Interface:** `action: { id, name, scope, projectLabel?, applicabilitySummary, updatedAt }`; `isSelected: boolean`; `onSelect: (id) => void`; `onOpenEditor: (id) => void`; `onLaunchWithAction: (id) => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only hover and action visibility state.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Selects the row; opens the action editor tab; opens the session launch modal prefilled from the selected action.
