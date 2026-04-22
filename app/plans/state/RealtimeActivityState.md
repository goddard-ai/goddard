# State Module: RealtimeActivityState

- **Responsibility:** Own the single authenticated realtime activity subscription and fan normalized events out to inbox, sessions, and pull request state without making each feature open its own stream.
- **Data Shape:** Subscription status, auth readiness, reconnect backoff metadata, last event timestamp or id, transient event buffer, and lightweight counters or diagnostics for consumers.
- **Mutations/Actions:** `connectActivityStream`; `disconnectActivityStream`; `handleIncomingEvent`; `acknowledgeEvent`; `markStreamError`; `retryStreamConnection`.
- **Scope & Hoisting:** Hoisted into a global provider because the stream is user-scoped and multiple feature areas need the same event feed.
- **Side Effects:** Opens the shared SDK-backed realtime stream for managed project-linked events; manages reconnect timers and teardown; forwards normalized events into `InboxState`, `SessionIndexState`, and `PullRequestState` through explicit cross-module actions instead of direct component coupling.
