# Component: NewSessionDialog

- **Minimum Viable Component:** Modal-only session launch flow for starting one new coding agent session with project, action, and initial prompt context.
- **Props Interface:** `isOpen: boolean`; `defaultProjectPath?: string | null`; `defaultActionId?: string | null`; `currentTabContext?: { kind, projectPath?, entityRef? } | null`; `onClose: () => void`; `onSubmitted: (sessionId) => void`.
- **Sub-components:** `SessionLaunchForm`, `ContextActionDropdown`.
- **State Complexity:** Owns its draft, validation, submit-pending, and reset-on-open behavior locally; only cross-surface launch defaults may be coordinated elsewhere.
- **Required Context:** `ProjectRegistryContext`, `ActionCatalogContext`.
- **Electrobun RPC:** None directly; session creation should route through shared SDK adapters or one small launch helper, not a dedicated form-state provider.
- **Interactions & Events:** Opens from the sessions page or a contextual action; edits launch inputs; submits a new session request; closes on success or cancellation.
