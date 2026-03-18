# PR Feedback One-Shot Daemon

## Goal

Use real-time managed-PR feedback to trigger focused, local one-shot `pi` sessions without requiring a human to monitor a live event feed.

## Hypothesis

We believe that immediate, automated handling of managed-PR comments and reviews will reduce reviewer wait time and improve PR throughput.

## Actors

- **Local Runtime Host** — desktop app-managed background worker or another supervised local process with repository access.
- **Authenticated Goddard User** — the developer identity that owns the daemon's stream and the managed PRs routed onto it.
- **Reviewer** — submits PR comments or reviews on GitHub.
- **Goddard GitHub App** — origin of managed PR metadata and webhook events.

## State Model

`Idle -> Connected -> EventReceived -> EligibilityChecked -> OneShotQueued -> OneShotRunning -> OneShotCompleted -> Connected`

## Core Behavior

- Each daemon process maintains one authenticated event stream for the current Goddard user.
- That stream may carry managed-PR feedback from multiple repositories when those PRs are owned by the current Goddard user.
- The runtime evaluates incoming events for one-shot eligibility and queues work by pull request, never by repository subscription boundaries.
- One-shot execution always uses the repository and pull request context carried by the event.
- After each one-shot completes, the runtime returns to connected listening mode.

## Hard Constraints

- Trigger only on PR comment and review feedback events.
- Consume a single authenticated stream per daemon process.
- React only to managed PRs owned by the authenticated Goddard user.
- Avoid overlapping one-shot execution for the same PR.
- Continue running until interrupted by the host supervisor.

## Failure Handling Expectations

- Stream disconnects should trigger reconnect attempts with bounded backoff.
- One-shot launch failures must be logged with PR context and must not crash the runtime.
- If multiple events arrive while a PR task is active, the runtime should coalesce or queue by PR and never run concurrently for the same PR.

## Non-Goals

- NON-GOAL: Implement long-running autonomous planning in this daemon.
- NON-GOAL: Serve as the primary human-facing workspace or review UI.
- NON-GOAL: Reintroduce a terminal-native GitHub review surface.

## Decision Memory

The daemon originally followed repository-scoped streams. That model no longer matched the actual ownership boundary for managed PR automation, so the daemon now follows the authenticated Goddard user and consumes one unified stream across repositories.
