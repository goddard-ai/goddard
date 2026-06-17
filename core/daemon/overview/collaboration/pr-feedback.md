# Pull Request Feedback

> Pull request feedback handling lets the local daemon react to supported review comments or reviews for the authenticated user. This page explains how feedback becomes queued local work without turning the daemon into a general review UI.

- **Core idea**
  - Pull request feedback handling is background daemon automation for supported review events.
  - It lets real-time comments or reviews trigger focused local handling without requiring a human to monitor a live event stream.

- **Event handling**
  - One daemon process maintains one authenticated event stream for the current user.
  - The stream may carry feedback from multiple repositories when the pull requests belong to the current user.
  - Supported feedback is queued by pull request.

- **Queueing**
  - The daemon avoids overlapping feedback handling for the same pull request.
  - Multiple events for one active pull request are coalesced or queued so handling remains sequential for that pull request.

- **Session context**
  - Each feedback session uses the repository and pull request context carried by the event.
  - Launch failures are reported with pull request context and should not crash the daemon runtime.

- **Boundaries**
  - Feedback handling is separate from workforce orchestration.
  - The runtime handles supported pull request comment and review feedback events; it is not broad long-running planning.
  - It is not a terminal-native replacement for a pull request review UI.
