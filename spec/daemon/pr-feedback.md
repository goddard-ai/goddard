# PR Feedback Flow

Real-time feedback can trigger focused local handling without requiring a human to monitor a live event feed.

## Participants
- Local Runtime Host — app-managed background worker or another supervised local process with repository access.
- Authenticated User — the developer identity that owns the stream and the pull requests routed onto it.
- Reviewer — submits pull request comments or reviews on GitHub.
- GitHub App — origin of managed metadata and webhook events.

## State Model

`Idle -> Connected -> EventReceived -> EligibilityChecked -> FeedbackQueued -> FeedbackHandling -> FeedbackHandled -> Connected`

## Capabilities
- Each daemon process maintains one authenticated event stream for the current user.
- That stream may carry feedback from multiple repositories when those pull requests are owned by the current user.
- The runtime evaluates incoming events for PR feedback eligibility and queues work by pull request, never by repository subscription boundaries.
- Each PR feedback session always uses the repository and pull request context carried by the event.
- After each PR feedback session completes, the runtime returns to connected listening mode.

## Boundaries
- Trigger only on pull request comment and review feedback events.
- Consume a single authenticated stream per daemon process.
- React only to pull requests owned by the authenticated user.
- Avoid overlapping PR feedback handling for the same pull request.
- Continue running until interrupted by host supervisor.
- Stream disconnects should trigger reconnect attempts with bounded backoff.
- PR feedback launch failures must be logged with pull request context and must not crash the runtime.
- If multiple events arrive while a pull request task is active, the runtime should coalesce or queue by pull request and never run concurrently for the same pull request.
- This runtime does not implement long-running autonomous planning.
- This runtime is not the primary human-facing workspace or review UI.
- This runtime must not reintroduce a terminal-native GitHub review surface.

## Rationale
This runtime originally followed repository-scoped streams. That model no longer matched the ownership boundary, so the daemon now follows the authenticated user and consumes one unified stream across repositories.
