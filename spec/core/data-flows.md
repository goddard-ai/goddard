# Data Flows

This file captures durable cross-component flow guarantees. Wire formats, API payloads, and internal execution steps belong in code.

## PR Creation (User-Initiated)

- Pull requests may be initiated from the app or another approved host.
- Pull request creation must pass through the platform's daemon and backend authority chain.
- The backend records enough ownership to route later feedback to the owning user.
- Reviewer feedback is routed through the authenticated stream for the owner.
- Receiving hosts update UI and local state from delivered events.

## Authentication (Lazy Device Flow)

- Desktop app and SDK hosts request authentication only when a protected action requires backend or external service identity.
- Authentication uses a device authorization challenge that can be completed in the user's browser.
- Hosts persist authorized session material using host-appropriate storage after completion.
- GitHub sign-in may serve as the user's visible identity, but backend-protected actions and streams use backend-issued session material.
- Backend session authorization is independent of any client-held GitHub credential and may be revoked or expired without requiring local GitHub token handling.

## Real-Time Event Subscription (Background Runtime)

- App and background hosts consume events through an authenticated user-scoped stream.
- Events owned by the authenticated user may arrive from multiple repositories over one stream.
- Unmanaged pull requests and events owned by other users must not be delivered on that stream.
- Local hosts may use delivered feedback events to update workspace state or launch PR feedback handling.
- A local host maintains the backend event stream only while at least one enabled capability can handle backend-originated events.
- When no enabled capability can handle backend-originated events, the host must not open the backend event stream and must close any active backend event stream.
- Backend stream health is a host-level runtime concern, separate from product behavior triggered by delivered events.

## Workforce Orchestration (Daemon-Owned)

- Workforce control starts through an approved client, but runtime ownership remains with the daemon.
- The daemon reconstructs workforce state from durable repository-local intent before admitting new work.
- Operators and agents may append delegated work through daemon controls.
- Each newly handled request runs in a fresh agent session.
- The daemon validates workforce changes, records them durably, and exposes current status to approved clients.

## Autonomous Cycle (Loop)

- A supervising runtime enforces cadence, throughput, and token constraints across autonomous cycles.
- The loop executes cycles through a persistent `pi-coding-agent` session.
- Per-cycle token limits are hard caps.
- Summary context carries forward between cycles until completion or termination.
