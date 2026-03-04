---
id: system-data-flows
status: ACTIVE
links:
  - type: Extends
    target: spec/architecture.md
  - type: Relates-To
    target: spec/cli/interactive.md
  - type: Relates-To
    target: spec/daemon/pr-feedback-one-shot.md
  - type: Relates-To
    target: spec/runtime-loop.md
  - type: Relates-To
    target: spec/rate-limiting.md
---

# Data Flows

This file captures conceptual end-to-end sequences only. Wire formats and API payload details belong in code.

## PR Creation (Interactive)

1. Developer runs `goddard pr create`.
2. CLI validates intent and forwards request through SDK.
3. Backend validates session and resolves GitHub identity.
4. Backend creates PR via GitHub App delegation and records managed-PR metadata.
5. Reviewer responds on GitHub (comment/review).
6. Webhook event enters backend and is routed to repository stream state.
7. Stream broadcast reaches subscribed clients.
8. SDK normalizes event; CLI renders feedback in terminal.

## Authentication (Device Flow)

1. CLI starts login and requests a device authorization challenge.
2. Backend creates a pending session.
3. CLI shows user code + verification URL.
4. User authorizes in browser.
5. Backend marks session authorized and stores identity.
6. CLI polling detects completion and persists token locally.

## Real-Time Event Subscription (Daemon)

1. Daemon subscribes to repository stream via SDK.
2. Backend validates session and attaches daemon connection to repo stream.
3. GitHub webhook events are routed to that repo stream.
4. SDK emits typed feedback events.
5. Daemon performs eligibility checks and may launch one-shot `pi` execution.

## Autonomous Cycle (Loop)

1. Loop enforces delay/throughput constraints.
2. Strategy generates next prompt from cycle context.
3. Loop executes prompt through persistent `pi-coding-agent` session.
4. Loop computes per-cycle token delta and enforces hard cap.
5. Loop updates summary context and decides continue vs `DONE` termination.
