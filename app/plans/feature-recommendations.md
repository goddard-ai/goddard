# Feature Recommendations

- Context:
  - This file tracks intentionally deferred follow-up work that is not already represented by a focused retained plan or implemented app surface.
  - Session launch, sessions, inbox, shortcuts, project management, basic settings, basic pull request detail, and session changes now exist in code, so future notes should describe what is still missing rather than restating those baselines.

- 1. Dedicated PR-Feedback Runtime Surface
  - Why deferred:
    - PR-feedback agent processes already surface in the session list, and the current MVP can route users into session and pull request tabs without a separate runtime page.
  - Potential future plans:
    - Components: `PrFeedbackRuntimePage`, `PrFeedbackEventQueue`, `PrFeedbackHealthBadge`
    - State: `PrFeedbackRuntimeState`

- 2. Workforce Orchestration UI
  - Why deferred:
    - `spec/daemon/workforce.md` explicitly says the workforce slice does not require a dedicated app UI, so it is not part of the MVP.
  - Potential future plans:
    - Components: `WorkforcePage`, `WorkforceRequestList`, `WorkforceRequestDetailView`
    - State: `WorkforceRuntimeState`

- 3. Expanded Project UX Beyond the Registry
  - Why deferred:
    - The app now has a project management page, but project home pages, status bars, or richer per-project dashboards remain undefined.
  - Potential future plans:
    - Components: `ProjectHomeView`, `ProjectStatusBar`

- 4. Configuration Editing Inside Settings
  - Why deferred:
    - `SettingsPage` exists for appearance, but operator-facing shared configuration editing and workspace preferences are not implemented.
  - Potential future plans:
    - Components: `ConfigScopeSwitcher`, `TextModelSelector`, `JsonConfigEditor`, `WorkspacePreferencesPanel`
    - State: `ConfigurationState`, `WorkspacePreferencesState`

- 5. Extension Catalog and Connectivity Diagnostics
  - Why deferred:
    - These remain lower-priority follow-ons after the core workspace flows exist.
  - Potential future plans:
    - Components: `ExtensionsPage`, `ConnectionStatusBanner`, `DiagnosticsPage`
    - State: `ExtensionCatalogState`, `ConnectivityState`

- 6. Terminal and Browser Preview Architecture Alignment
  - Why deferred:
    - The existing terminal and preview plans still assume bespoke host support that has not yet been aligned with the current Electrobun boundary described in `spec/app.md` and `app/AGENTS.md`.
  - Potential future plans:
    - Either update the spec to explicitly allow the required Electrobun host capabilities or redesign those features around the current Electrobun boundary before implementation begins.
