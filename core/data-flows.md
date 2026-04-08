# Data Flows

This file captures conceptual end-to-end sequences only. Wire formats and API payload details belong in code.

## PR Creation (User-Initiated)

1. Developer initiates pull request creation from the desktop app or another approved host.
2. The host forwards the request through the platform's daemon and backend authority chain.
3. Backend validates the session, resolves GitHub identity, and records which Goddard user owns the managed pull request.
4. Backend creates the pull request through delegated GitHub authority and persists enough managed pull request identity to pair later feedback with the owning Goddard user.
5. Reviewer responds on GitHub with comments or review feedback.
6. Webhook event enters the backend.
7. Backend determines whether the referenced pull request is managed and, if so, which Goddard user owns it.
8. Event delivery is routed onto that user's authenticated stream.
9. The receiving host updates UI and local state from the delivered event.

## Authentication (Lazy Device Flow)

1. Desktop app or SDK host requests a protected action and starts a device authorization challenge.
2. Backend creates a pending session.
3. Host presents the user code and verification URL.
4. User authorizes in browser.
5. Backend marks session authorized and stores identity.
6. Host detects completion and persists token using host-appropriate storage.

## Real-Time Event Subscription (Background Runtime)

1. Desktop app or background runtime opens an authenticated managed pull request event stream as part of its local host responsibilities.
2. Backend validates the session and attaches the subscriber connection to the current Goddard user's stream.
3. Managed pull request events owned by that user may arrive from multiple repositories over the same stream.
4. Unmanaged pull request events and events owned by other Goddard users are not delivered on that stream.
5. The local host interprets the delivered feedback event for its own state and automation decisions.
6. The subscriber updates workspace state or may launch the PR feedback flow.

## Workforce Orchestration (Daemon-Owned)

1. Operator initializes repository-local workforce intent and starts workforce control through an approved client.
2. Daemon resolves the target repository workspace and reconstructs current workforce state from durable intent.
3. Operator or agent appends new delegated work to the repository workforce.
4. Daemon projects the new intent into current queue state and selects the next eligible work per agent.
5. Daemon launches a fresh agent session for each newly handled request.
6. Active agents respond, suspend, or delegate additional work through daemon-backed workforce controls.
7. Daemon validates those changes, records them durably, and updates projected status.
8. Operators and clients inspect current workforce status through the daemon.

## Autonomous Cycle (Loop)

1. A supervising runtime enforces delay and throughput constraints.
2. Strategy generates the next prompt from cycle context.
3. Runtime executes the prompt through a persistent `pi-coding-agent` session.
4. Runtime computes per-cycle token delta and enforces the hard cap.
5. Runtime updates summary context and decides continue vs `DONE` termination.
