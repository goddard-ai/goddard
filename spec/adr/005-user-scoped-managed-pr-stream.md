# ADR-005: Managed PR Event Delivery Uses User-Scoped SSE Streams

## Status
ACTIVE

## Context

The original stream model attached subscribers to repositories. That model no longer matched the product boundary for automation: managed PR feedback belongs to the authenticated Goddard user who initiated the managed PR, and a single daemon process may need feedback from many repositories at once.

GitHub author identity is also not a reliable routing boundary. The backend already owns the managed-PR lifecycle, so it is the authoritative place to remember which Goddard user initiated a managed PR and should receive its later feedback events.

## Decision

Managed PR event delivery uses authenticated, user-scoped **Server-Sent Events (SSE)** streams.

Each subscriber opens one long-lived stream for the current Goddard user. The backend routes PR-created events and later webhook feedback by managed-PR ownership. Repository membership alone does not determine delivery, and GitHub author identity does not override Goddard ownership.

## Rationale

- **Matches the automation actor:** Background automation is owned by an authenticated developer, not by a repository subscription list.
- **Reduces client coordination:** SDK consumers and daemons maintain one stream instead of tracking repository-by-repository subscriptions.
- **Preserves isolation:** User-scoped routing prevents managed PR feedback from leaking between Goddard users.
- **Keeps the transport simple:** The stream remains one-way server-to-client traffic, so SSE continues to fit the delivery model.

## Consequences

- The backend must persist managed-PR ownership when a PR is created so later feedback can be routed correctly.
- SDK, desktop, and daemon consumers subscribe once per authenticated user session rather than once per repository.
- Unmanaged PRs are not delivered on the managed stream.
- Delivery guarantees apply to managed PRs whose ownership was recorded under this routing model; older records outside that guarantee boundary are not promised stream delivery.
