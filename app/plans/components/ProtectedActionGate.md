# Component: ProtectedActionGate

- **Minimum Viable Component:** Reusable wrapper that blocks protected actions until the user is authenticated, then resumes the requested action automatically.
- **Props Interface:** `isAuthenticated: boolean`; `actionLabel: string`; `onAllowed: () => void`; `onRequireAuth: () => void`; `children: preact component tree`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only pending-intent presentation; queued protected action intent belongs in `AuthState`.
- **Required Context:** `AuthContext`.
- **Electrobun RPC:** None.
- **Interactions & Events:** Intercepts click or submit events; opens auth flow when needed; replays the deferred callback after successful authentication.
