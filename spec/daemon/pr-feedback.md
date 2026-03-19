# PR Feedback One-Shot Runtime

## Goal
Use real-time managed pull request feedback to trigger focused, local one-shot `pi` sessions without requiring a human to monitor a live event feed.

## Hypothesis
We believe that immediate, automated handling of managed pull request comments and reviews will reduce reviewer wait time and improve pull request throughput.

## Actors
- Local Runtime Host — desktop app-managed background worker or another supervised local process with repository access.
- Authenticated Goddard User — the developer identity that owns the daemon's stream and the managed pull requests routed onto it.
- Reviewer — submits pull request comments or reviews on GitHub.
- Goddard GitHub App — origin of managed pull request metadata and webhook events.

## State Model

`Idle -> Connected -> EventReceived -> EligibilityChecked -> OneShotQueued -> OneShotRunning -> OneShotCompleted -> Connected`

## Core Behavior
- Each daemon process maintains one authenticated event stream for the current Goddard user.
- That stream may carry managed pull request feedback from multiple repositories when those pull requests are owned by the current Goddard user.
- The runtime evaluates incoming events for one-shot eligibility and queues work by pull request, never by repository subscription boundaries.
- One-shot execution always uses the repository and pull request context carried by the event.
- After each one-shot completes, the runtime returns to connected listening mode.

## Hard Constraints
- Trigger only on pull request comment and review feedback events.
- Consume a single authenticated stream per daemon process.
- React only to managed pull requests owned by the authenticated Goddard user.
- Avoid overlapping one-shot execution for the same pull request.
- Continue running until interrupted by host supervisor.

## Failure Handling Expectations
- Stream disconnects should trigger reconnect attempts with bounded backoff.
- One-shot launch failures must be logged with pull request context and must not crash the runtime.
- If multiple events arrive while a pull request task is active, the runtime should coalesce or queue by pull request and never run concurrently for the same pull request.

## Non-Goals
- Implementing long-running autonomous planning in this runtime
- Serving as the primary human-facing workspace or review UI
- Reintroducing a terminal-native GitHub review surface

## Decision Memory
This runtime originally followed repository-scoped streams. That model no longer matched the actual ownership boundary for managed pull request automation, so the daemon now follows the authenticated Goddard user and consumes one unified stream across repositories.
