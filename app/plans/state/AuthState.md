# State Module: AuthState

- **Responsibility:** Own lazy authentication, protected-action gating, device-flow progress, authenticated identity, and logout behavior for the desktop app.
- **Data Shape:** Current auth status; authenticated identity summary; pending protected action intent; active device-flow challenge metadata; polling status; error state; token persistence status; derived booleans for managed features being available.
- **Mutations/Actions:** `hydrateAuthSession`; `beginProtectedAction`; `startDeviceFlow`; `pollDeviceFlow`; `completeAuth`; `cancelDeviceFlow`; `logout`; `retryProtectedAction`; `clearAuthError`.
- **Scope & Hoisting:** Hoisted into a global provider because auth affects realtime subscriptions, pull request actions, loop controls, and other protected workflows across the app.
- **Side Effects:** Uses SDK-backed daemon auth flows; persists the auth session to host storage; coordinates with `RealtimeActivityState` so the shared event stream starts only when the user is authenticated.
