# State Module: ActionCatalogState

- **Responsibility:** Own the list of global and project-scoped actions, their filter state, applicability to the current tab, and the dropdown-ready view of that catalog.
- **Data Shape:** Normalized action records keyed by action id; filter fields for scope, project, applicability, and query; current tab context; derived grouped lists for management pages and contextual dropdowns; load and refresh state.
- **Mutations/Actions:** `loadActions`; `refreshActions`; `setActionFilters`; `setCurrentTabContext`; `selectAction`; `clearActionSelection`; `openManageActionsPage`.
- **Scope & Hoisting:** Hoisted into a global provider because the action catalog is reused by the actions page, the session launch dialog, and contextual action dropdowns across many tabs.
- **Side Effects:** Reads action definitions from shared configuration roots and action sources; keeps global and project-local action records aligned with the same underlying configuration model expected by the app and SDK.
