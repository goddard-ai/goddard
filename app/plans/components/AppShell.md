# Component: AppShell
- **Minimum Viable Component:** Composition root for the desktop shell that mounts provider boundaries, renders the icon sidebar, renders the primary workbench view, and hosts the closable detail tab area.
- **Props Interface:** `services: { sdkAdapters, daemonAdapters, persistenceAdapter, tauriCommandAdapter }`; `initialWorkspaceSnapshot?: { selectedNavId, openTabs, activeTabId }`; `featureFlags?: record of boolean flags`.
- **Sub-components:** `SidebarNav`, `MainWorkbenchView`, `WorkbenchTabs`.
- **State Complexity:** Simple local layout state only; all domain and persistence state should be isolated in `preact-sigma` modules.
- **Required Context:** None directly; this component is the provider composition boundary for navigation, tabs, realtime activity, sessions, inbox, documents, diffs, chat, terminal, and preview state.
- **Tauri IPC:** No direct commands; IPC-capable state modules and service adapters are injected from here.
- **Interactions & Events:** Bootstraps app hydration; wires provider lifecycles; passes navigation and tab events down; surfaces unrecoverable app-level loading and error states.
