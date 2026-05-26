# Desktop App Workflows

## Goal
Define the human-facing workflows the desktop app supports across sessions, reviews, specs, tasks, roadmap context, and discovery.

## Hypothesis
We believe consolidating the core Goddard workflows into one visual surface will make AI-assisted delivery easier to steer, review, and prioritize.

## Actors
- Developer/operator initiating and steering work
- Reviewer giving feedback on AI output
- Maintainer triaging progress, blockers, and upcoming work

## Core Capabilities
- **Session Steering**: Initiate, monitor, and provide real-time feedback to AI agents executing tasks.
- **Human Attention Inbox**: Triage daemon-managed sessions and pull requests that currently need review, response, or explicit completion.
- **Pull Request Review**: Triage, review, and correlate AI-generated pull requests directly with their originating sessions.
- **Specification Management**: Browse and refine repository specifications to align human intent with AI execution.
- **Task & Roadmap Prioritization**: View and manage the queue of upcoming work and long-term proposals.
- **Global Discovery**: Search across all domains from a single entry point.

## Behavior Model
- The app exposes real-time state for active sessions, tasks, pull requests, and proposals.
- The app surfaces daemon-owned inbox state without creating a separate app-owned source of truth.
- The app allows humans to monitor, review, and adjust AI execution without dropping context.

## State Machines
- **Session Lifecycle View**: `Idle -> Active -> Blocked (Awaiting Input) -> Completed`

## Constraints
- Human-facing day-to-day workflows belong in the desktop app rather than in a broad terminal-first surface.
- Shared data loading, mutation, and system configuration behavior must remain aligned with the SDK.

## Non-Goals
- Reintroducing command-based authentication, pull request creation, spec editing, proposal review, or other broad product workflows as primary CLI flows.
- Defining exact screens, routes, component layouts, or local storage mechanics.
