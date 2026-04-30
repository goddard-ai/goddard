# Desktop App Intent Index

## Goal
Provide a unified desktop workspace for Goddard operations so developers can run sessions, review outputs, and steer work from one visual surface instead of fragmented repository, GitHub, and chat tooling.

## Hypothesis
We believe that consolidating sessions, pull requests, specs, tasks, and roadmap context into one desktop app will reduce context switching and speed up AI-assisted delivery.

## Big Picture
The desktop app is the primary human-facing interface to the Goddard runtime. It is not a forked product surface with separate logic. The app should reflect the same daemon-backed control surfaces and backend-owned real-time activity used by other platform consumers.

## Primary Actors
- Developer/operator managing one or more repositories
- Reviewer giving feedback on AI output
- Maintainer monitoring throughput and blockers

## Global UI Model
- IDE-like shell with:
  - Persistent left navigation icons by domain
  - Persistent non-closable Main Tab
  - Additional closable detail tabs (max 20 concurrent)
- Navigation icon selection updates Main Tab content.
- Drill-down interactions open domain-specific detail tabs.

## Core Capabilities
- **Session Steering**: Initiate, monitor, and provide real-time feedback to AI agents executing tasks.
- **Human Attention Inbox**: Triage daemon-managed sessions and pull requests that currently need review, response, or explicit completion.
- **Pull Request Review**: Triage, review, and correlate AI-generated pull requests directly with their originating sessions.
- **Specification Management**: Browse and refine repository specifications to align human intent with AI execution.
- **Task & Roadmap Prioritization**: View and manage the queue of upcoming work and long-term proposals.
- **Global Discovery**: Search across all domains (sessions, PRs, specs, tasks) from a single entry point.

## Behavior Model
- **Centralized Context**: Provides a single pane of glass for all repository-specific AI operations.
- **Real-Time Visibility**: Exposes real-time state of active sessions, tasks, and proposals.
- **Attention Triage**: Surfaces daemon-owned inbox state without creating a separate app-owned source of truth.
- **Interactive Steering**: Allows humans to seamlessly monitor, review, and adjust AI execution without dropping context.

## State Machines
- **Authentication Flow**: `Anonymous -> Authenticated Action Requested -> Auth Prompt -> Authenticated` (Authentication is lazy; users are only prompted to log in when attempting an action that requires a backend or external service identity, such as GitHub).
- **Session Lifecycle View**: `Idle -> Active -> Blocked (Awaiting Input) -> Completed`

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
- User workspace preferences (filters, recents)

## Cross-Cutting Constraints
- Must remain lightweight.
- Frontend-Heavy Architecture: The application should keep domain behavior in the visual workspace and route privileged local integrations through a minimal trusted desktop host boundary.
- Trusted Host Boundary: Embedded browser surfaces must access privileged local capabilities through the trusted desktop host. They must not connect directly to daemon IPC unless a future browser-safe daemon contract explicitly defines origin, authentication, and lifecycle guarantees.
- Lazy Authentication: The application must function in a degraded or local-only mode until an external service (like GitHub) is explicitly requested.
- Must handle streaming updates gracefully for high-churn views.

## Non-Goals
- Reintroducing a broad parallel CLI or other terminal-first primary workflow surface.
- Implementing a full in-app code editor.
