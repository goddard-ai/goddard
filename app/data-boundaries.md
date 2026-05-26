# Desktop App Data And Boundaries

The app consumes shared platform data and routes privileged behavior through trusted host boundaries. It is the primary human workspace, not a separate source of platform truth.

## Participants
- Developer/operator using local and authenticated features
- Desktop host providing privileged local capabilities
- Embedded browser surface presenting the workspace UI
- SDK and daemon boundaries providing shared behavior

## Shared Data
All screens consume normalized, real-time domain records with stable identities:
- Repository
- Session
- Pull request
- Inbox item
- Message/activity event
- Task
- Roadmap proposal
- Spec file metadata/content
- Page metadata/content
- Extension metadata
- User workspace preferences

## Lazy Authentication
`Anonymous -> Authenticated Action Requested -> Auth Prompt -> Authenticated`

Authentication is lazy. Users are only prompted to log in when attempting an action that requires a backend or external service identity, such as GitHub.

## Boundaries
- The application should keep domain behavior in the visual workspace and route privileged integrations through a minimal trusted host boundary.
- Embedded browser surfaces must access privileged local capabilities through the trusted desktop host.
- Embedded browser surfaces must not connect directly to daemon IPC.
- The application must function in a degraded or local-only mode until an external service is explicitly requested.
- High-churn views must handle streaming updates gracefully.
- The app must not invent a parallel configuration model.
- The app must not create a separate source of truth for inbox, session, or runtime state.
- App behavior that depends on shared data loading, shared data mutation, or system configuration must have SDK parity.
- This spec does not define API payloads, database schemas, daemon IPC contracts, or browser-host bridge implementation details.
- Local-only degraded behavior is not a separate product mode.
