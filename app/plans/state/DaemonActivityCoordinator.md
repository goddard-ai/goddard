# State Module: DaemonActivityCoordinator

- **Current Baseline:** `app/src/shared/daemon-subscriptions.ts` already provides the browser-side daemon stream subscription coordinator for concrete streams such as session messages and workforce events.
- **Responsibility:** Coordinate a future user-scoped or project-scoped activity stream only if multiple app features need one shared feed that cannot be represented as feature-local subscriptions plus query invalidation.
- **Data Shape:** Subscription status, auth readiness, reconnect backoff metadata, last event timestamp or id, transient diagnostics, and optional counters for feature consumers.
- **Mutations/Actions:** `connectActivityStream`; `disconnectActivityStream`; `handleIncomingEvent`; `markStreamError`; `retryStreamConnection`; `clearActivityDiagnostics`.
- **Scope & Hoisting:** Hoist globally only for a truly shared stream. Keep session, inbox, and other feature-specific streams owned by their feature code when they have independent lifetimes.
- **Side Effects:** Use the existing daemon subscription coordinator rather than opening ad hoc browser-host channels. Fan events into feature models such as `Inbox` or into `queryClient` invalidation for SDK-backed reads; do not reference deleted plan-only owners such as `SessionIndexState`.
- **Auth Boundary:** Start authenticated streams only after auth is ready, and tear them down on logout or webview reset.
