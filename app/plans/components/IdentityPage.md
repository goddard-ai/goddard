# Component: IdentityPage
- **Minimum Viable Component:** Full-width account and auth status page that explains degraded local-only mode, shows the current authenticated identity when present, and exposes login and logout actions.
- **Props Interface:** `class?: string`; `showProtectedActionsSummary?: boolean`.
- **Sub-components:** `ProtectedActionGate`, `DeviceFlowDialog`.
- **State Complexity:** Simple UI-only section expansion and empty-state presentation; auth lifecycle state belongs in `AuthState`.
- **Required Context:** `AuthContext`.
- **Electrobun RPC:** None directly; auth operations should route through `AuthState`.
- **Interactions & Events:** Starts device-flow auth; retries the most recent protected action after login; logs out; shows whether realtime and managed PR features are available.
