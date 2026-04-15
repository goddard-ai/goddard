# Feature Recommendations

- Context:
  - This file now tracks intentionally deferred follow-up work that is not yet represented by dedicated plan files.
  - MVP plans now exist for auth, project management, session launch, actions, pull request triage and compose, global search, spec discovery, task lists, roadmap lists, and loop management.

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

- 4. Configuration and Settings Editing
  - Why deferred:
    - You explicitly punted on settings for now, even though `spec/configuration.md` implies a future need for operator-facing config editing and workspace preferences.
  - Potential future plans:
    - Components: `SettingsPage`, `ConfigScopeSwitcher`, `TextModelSelector`, `JsonConfigEditor`, `WorkspacePreferencesPanel`
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
