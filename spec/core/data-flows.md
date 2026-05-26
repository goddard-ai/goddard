# Data Flows

This file captures durable cross-component flow guarantees. Wire formats, API payloads, and internal execution steps belong in code.

## PR Creation (User-Initiated)

- Pull requests may be initiated from the desktop app or another approved host.
- Pull request creation must pass through the platform's daemon and backend authority chain.
- The backend records enough managed pull request ownership to route later feedback to the owning Goddard user.
- Reviewer feedback on managed pull requests is routed through the authenticated stream for the owning Goddard user.
- Receiving hosts update UI and local state from delivered managed pull request events.

## Authentication (Lazy Device Flow)

- Desktop app and SDK hosts request authentication only when a protected action requires backend or external service identity.
- Authentication uses a device authorization challenge that can be completed in the user's browser.
- Hosts persist authorized session material using host-appropriate storage after completion.

## Real-Time Event Subscription (Background Runtime)

- Desktop app and background runtime hosts consume managed pull request events through an authenticated user-scoped stream.
- Managed pull request events owned by the authenticated Goddard user may arrive from multiple repositories over one stream.
- Unmanaged pull requests and events owned by other Goddard users must not be delivered on that stream.
- Local hosts may use delivered feedback events to update workspace state or launch PR feedback handling.

## Workforce Orchestration (Daemon-Owned)

- Workforce control starts through an approved client, but runtime ownership remains with the daemon.
- The daemon reconstructs workforce state from durable repository-local intent before admitting new work.
- Operators and agents may append delegated work through daemon-backed workforce controls.
- Each newly handled request runs in a fresh agent session.
- The daemon validates workforce changes, records them durably, and exposes current status to approved clients.

## Autonomous Cycle (Loop)

- A supervising runtime enforces cadence, throughput, and token constraints across autonomous cycles.
- The loop executes cycles through a persistent `pi-coding-agent` session.
- Per-cycle token limits are hard caps.
- Summary context carries forward between cycles until completion or termination.
