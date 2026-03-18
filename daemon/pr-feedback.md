# PR Feedback One-Shot Runtime

## Goal
Use real-time repository feedback to trigger focused, local one-shot `pi` sessions without requiring a human to monitor a live event feed.

## Hypothesis
We believe that immediate, automated handling of managed-PR comments and reviews will reduce reviewer wait time and improve pull request throughput.

## Actors
- Local Runtime Host — desktop app-managed background worker or another supervised local process with repository access.
- Reviewer — submits pull request comments or reviews on GitHub.
- Goddard GitHub App — origin of managed pull request metadata and webhook events.

## State Model

`Idle -> Subscribed -> EventReceived -> EligibilityChecked -> OneShotQueued -> OneShotRunning -> OneShotCompleted -> Idle`

## Core Behavior
1. Background runtime subscribes to repository stream events through SDK.
2. On feedback events, runtime checks whether the pull request is Goddard-managed.
3. Eligible events enqueue one one-shot task per pull request.
4. Task launches local `pi` with repository, pull request, and reviewer feedback context.
5. Prompt contract requires the session to conclude by posting a pull request thread response.
6. Runtime returns to subscribed mode and continues event processing.

## Hard Constraints
- Trigger only on pull request comment and review feedback events.
- Ignore non-managed pull requests.
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
This runtime moved under daemon ownership because repository feedback is an automation trigger, not primarily interactive output.
