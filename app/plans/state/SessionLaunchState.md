# State Module: SessionLaunchState
- **Responsibility:** Coordinate cross-surface requests to open one shared session-launch dialog with project, action, or tab-context defaults. Do not own the dialog’s editable form draft.
- **Data Shape:** Dialog open state; launch source metadata; `defaultProjectPath`; `defaultActionId`; `currentTabContext`; one reset token or equivalent so reopening with new defaults can replace the local dialog draft cleanly.
- **Mutations/Actions:** `openLaunchDialog`; `closeLaunchDialog`; `setLaunchDefaults`; `clearLaunchDefaults`.
- **Scope & Hoisting:** Optional and intentionally thin. Only hoist this if multiple surfaces need to target one shared dialog host; keep prompt text, validation errors, and submit-pending state inside `NewSessionDialog`.
- **Side Effects:** None required for the MVP beyond surfacing one normalized open request to the dialog host.
