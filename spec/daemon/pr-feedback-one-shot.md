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
---

# PR Feedback One-Shot Daemon

## Goal

Use the real-time repository stream to autonomously react to PR feedback without requiring a human to run a dedicated stream command.

## Hypothesis

We believe that running a local daemon that listens for PR comment/review events and launches a targeted one-shot `pi` session will reduce PR turnaround time and keep AI-driven PRs responsive to reviewer input.

## Actors

- **Daemon Operator** — runs the daemon process on a machine with repository access.
- **Reviewer** — leaves a PR comment or review on GitHub.
- **Goddard GitHub App** — source of managed PRs and webhook events.

## State Model

`Idle -> Subscribed -> EventReceived -> EligibilityChecked -> OneShotRunning -> OneShotCompleted -> Idle`

## Behavior

1. Daemon subscribes to repository stream events through the SDK.
2. On `comment` or `review` events, daemon checks whether the PR is managed by Goddard GitHub App.
3. If managed, daemon starts a local one-shot `pi` session with context about repo, PR number, and reviewer feedback.
4. Session prompt must instruct the AI to finish by replying on the responsible PR thread.
5. Daemon returns to subscribed state and continues processing subsequent events.

## Constraints

- Must only trigger on comment/review feedback events.
- Must ignore PRs that are not managed by Goddard.
- Must run continuously until interrupted.
- Must avoid overlapping one-shot runs for the same PR.

## Non-Goals

- NON-GOAL: Implement a long-running autonomous planning loop in this daemon.
- NON-GOAL: Replace existing `goddard loop` workflows.
- NON-GOAL: Build custom GitHub review UI in terminal.

## Decision Memory

- Pivoted from exposing event streaming as a human-facing CLI command to daemon consumption, because stream events are primarily operational inputs for automated PR follow-up.
