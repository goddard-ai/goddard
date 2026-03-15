# Data Flows

This file captures conceptual end-to-end sequences only. Wire formats and API payload details belong in code.

## PR Creation (User-Initiated)

1. Developer initiates PR creation from the desktop app or an SDK-powered host.
2. SDK validates intent and forwards request through platform contracts.
3. Backend validates session and resolves GitHub identity.
4. Backend creates PR via GitHub App delegation and records managed-PR metadata.
5. Reviewer responds on GitHub (comment/review).
6. Webhook event enters backend and is routed to repository stream state.
7. Stream broadcast reaches subscribed clients.
8. SDK normalizes the event; the desktop app or host updates UI and local state.

## Authentication (Lazy Device Flow)

1. Desktop app or SDK host requests a protected action and starts a device authorization challenge.
2. Backend creates a pending session.
3. Host presents the user code and verification URL.
4. User authorizes in browser.
5. Backend marks session authorized and stores identity.
6. Host detects completion and persists token using host-appropriate storage.

## Real-Time Event Subscription (Background Runtime)

1. Desktop app or background runtime subscribes to repository stream via SDK.
2. Backend validates session and attaches the subscriber connection to repo stream.
3. GitHub webhook events are routed to that repo stream.
4. SDK emits typed feedback events.
5. Subscriber updates workspace state or may launch one-shot `pi` execution.

## Autonomous Cycle (Loop)

1. A supervising runtime enforces delay and throughput constraints.
2. Strategy generates next prompt from cycle context.
3. Runtime executes prompt through persistent `pi-coding-agent` session.
4. Runtime computes per-cycle token delta and enforces hard cap.
5. Runtime updates summary context and decides continue vs `DONE` termination.
