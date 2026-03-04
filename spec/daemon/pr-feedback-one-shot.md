---
id: daemon-pr-feedback-one-shot
status: ACTIVE
links:
  - type: Extends
    target: spec/daemon/index.md
  - type: Depends-On
    target: spec/data-flows.md
  - type: Relates-To
    target: spec/cli/interactive.md
  - type: Relates-To
    target: spec/non-goals.md
---

# PR Feedback One-Shot Daemon

## Goal

Use real-time repository feedback to trigger focused, local one-shot `pi` sessions without manual stream monitoring.

## Hypothesis

We believe that immediate, automated handling of managed-PR comments/reviews will reduce reviewer wait time and improve PR throughput.

## Actors

- **Daemon Operator** — runs daemon on a machine with repository access.
- **Reviewer** — submits PR comments or reviews on GitHub.
- **Goddard GitHub App** — origin of managed PR metadata and webhook events.

## State Model

`Idle -> Subscribed -> EventReceived -> EligibilityChecked -> OneShotQueued -> OneShotRunning -> OneShotCompleted -> Idle`

## Core Behavior

1. Daemon subscribes to repository stream events through SDK.
2. On feedback events, daemon checks whether PR is Goddard-managed.
3. Eligible events enqueue one one-shot task per PR.
4. Task launches local `pi` with repository, PR number, and reviewer feedback context.
5. Prompt contract requires the session to conclude by posting a PR-thread response.
6. Daemon returns to subscribed mode and continues event processing.

## Hard Constraints

- Trigger only on PR comment/review feedback events.
- Ignore non-managed PRs.
- Avoid overlapping one-shot execution for the same PR.
- Continue running until interrupted by operator or host supervisor.

## Failure Handling Expectations

- Stream disconnects should trigger reconnect attempts with bounded backoff.
- One-shot launch failures must be logged with PR context and must not crash daemon process.
- If multiple events arrive while a PR task is active, daemon should coalesce or queue by PR (never run concurrently for same PR).

## Non-Goals

- NON-GOAL: Implement long-running autonomous planning in this daemon.
- NON-GOAL: Replace `goddard loop` workflows.
- NON-GOAL: Provide a terminal-native GitHub review UI.

## Decision Memory

Pivoted from a human-facing stream command to daemon ownership because stream events are operational automation triggers, not primarily interactive output.
