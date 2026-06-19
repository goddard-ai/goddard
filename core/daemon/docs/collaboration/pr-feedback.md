# Pull Request Feedback

> Pull request feedback handling lets the local daemon react to supported review comments or reviews for the authenticated user. This page explains how feedback becomes queued local work without turning the daemon into a general review UI.

## Core idea

- Pull request feedback handling is background daemon automation for supported review events.
- It lets real-time comments or reviews trigger focused local handling without requiring a human to monitor a live event stream.

## Event handling

- One daemon process maintains one authenticated event stream for the current user.
- The stream may carry feedback from multiple repositories when the pull requests belong to the current user.
- Supported feedback is queued by pull request.
- Unsupported or unrelated events should not create local work just because they appeared on the stream.
- If the authenticated stream is interrupted, the daemon should recover by resuming supported handling rather than letting clients invent feedback state.

## Queueing

- The daemon avoids overlapping feedback handling for the same pull request.
- Multiple events for one active pull request are coalesced or queued so handling remains sequential for that pull request.
- Queueing preserves the user's review context: the next handler sees pull request context instead of an isolated comment with no local workflow.
- Events for different pull requests may be observed by the same daemon, but each pull request keeps its own sequential handling boundary.

## Session context

- Each feedback session uses the repository and pull request context carried by the event.
- Launch failures are reported with pull request context and should not crash the daemon runtime.
- The resulting session should be inspectable like other daemon sessions, including history and diagnostics when launch succeeds.

## Boundaries

- Feedback handling is separate from workforce orchestration.
- The runtime handles supported pull request comment and review feedback events; it is not broad long-running planning.
- It is not a terminal-native replacement for a pull request review UI.
