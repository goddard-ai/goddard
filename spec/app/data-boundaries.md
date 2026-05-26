# Desktop App Data And Boundaries

## Goal
Define the shared data and trust boundaries that let the desktop app act as a primary human workspace without becoming a separate source of platform truth.

## Hypothesis
We believe the desktop app can stay responsive and useful while preserving platform consistency by consuming normalized shared records and routing privileged behavior through trusted host boundaries.

## Actors
- Developer/operator using local and authenticated workspace features
- Desktop host providing privileged local capabilities
- Embedded browser surface presenting the workspace UI
- SDK and daemon boundaries providing shared platform behavior

## Shared Data Requirements
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

## Authentication Flow
`Anonymous -> Authenticated Action Requested -> Auth Prompt -> Authenticated`

Authentication is lazy. Users are only prompted to log in when attempting an action that requires a backend or external service identity, such as GitHub.

## Boundaries
- The application should keep domain behavior in the visual workspace and route privileged local integrations through a minimal trusted desktop host boundary.
- Embedded browser surfaces must access privileged local capabilities through the trusted desktop host.
- Embedded browser surfaces must not connect directly to daemon IPC.
- The application must function in a degraded or local-only mode until an external service is explicitly requested.
- High-churn views must handle streaming updates gracefully.

## Constraints
- The app must not invent a parallel configuration model.
- The app must not create a separate source of truth for daemon-owned inbox, session, or runtime state.
- App behavior that depends on shared data loading, shared data mutation, or system configuration must have SDK parity.

## Non-Goals
- Defining API payloads, database schemas, daemon IPC contracts, or browser-host bridge implementation details.
- Treating local-only degraded behavior as a separate product mode.
