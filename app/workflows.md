# Desktop App Workflows

The app supports workflows across sessions, reviews, specs, tasks, roadmap context, and discovery.

## Users
- Developer/operator initiating and steering work
- Reviewer giving feedback on AI output
- Maintainer triaging progress, blockers, and upcoming work

## Capabilities
- **Session Steering**: Initiate, monitor, and provide real-time feedback to AI agents executing tasks.
- **Human Attention Inbox**: Triage sessions and pull requests that need review, response, or explicit completion.
- **Pull Request Review**: Triage, review, and correlate AI-generated pull requests directly with their originating sessions.
- **Specification Management**: Browse and refine repository specifications to align human intent with AI execution.
- **Task & Roadmap Prioritization**: View and manage the queue of upcoming work and long-term proposals.
- **Global Discovery**: Search across all domains from a single entry point.
- The app exposes real-time state for active sessions, tasks, pull requests, and proposals.
- The app surfaces daemon-owned inbox state without creating a separate app-owned source of truth.
- The app allows humans to monitor, review, and adjust AI execution without dropping context.

## Lifecycle
- **Session Lifecycle View**: `Idle -> Active -> Blocked (Awaiting Input) -> Completed`

## Boundaries
- Day-to-day workflows belong here rather than in a broad terminal-first surface.
- Shared data loading, mutation, and system configuration behavior must remain aligned with the SDK.
- The app must not reintroduce command-based authentication, pull request creation, spec editing, proposal review, or other broad product workflows as primary CLI flows.
- This spec does not define exact screens, routes, component layouts, or local storage mechanics.
