# State Module: SessionLaunchState
- **Responsibility:** Manage the modal-only session launch workflow, including repository selection, action-prefill, initial prompt text, validation, and submission.
- **Data Shape:** Dialog open state; launch draft fields; current tab context; resolved applicable actions; validation errors; submission status; created session id; transient launch error.
- **Mutations/Actions:** `openLaunchDialog`; `closeLaunchDialog`; `setLaunchDraft`; `prefillFromAction`; `prefillFromTabContext`; `validateLaunchDraft`; `submitLaunch`; `clearLaunchError`.
- **Scope & Hoisting:** Hoisted into a shared provider because the launch modal can be opened from the sessions page, action management, and contextual tab-level action menus.
- **Side Effects:** Resolves repository and action defaults from other state modules; creates daemon-backed sessions through shared SDK adapters; opens the resulting session chat tab through `WorkbenchTabsState`.
