# ADR-004: Managed Pull Request Event Delivery Uses User-Scoped NDJSON Streams

## Status
ACTIVE

## Context

The original stream model attached subscribers to repositories. That model no longer matched the product boundary for automation: managed pull request feedback belongs to the authenticated Goddard user who initiated the managed pull request, and a single daemon process may need feedback from many repositories at once.

GitHub author identity is also not a reliable routing boundary. The backend already owns the managed pull request lifecycle, so it is the authoritative place to remember which Goddard user initiated a managed pull request and should receive its later feedback events.

Live delivery remains one-way server-to-client traffic, but the transport should stay easy for SDK, desktop, and daemon consumers to read from ordinary streaming HTTP responses without binding the product contract to browser-specific event framing.

## Decision

Managed pull request event delivery uses authenticated, user-scoped **NDJSON** streams over long-lived HTTP responses.

Each subscriber opens one long-lived stream for the current Goddard user. The backend routes pull request creation events and later webhook feedback by managed pull request ownership. Repository membership alone does not determine delivery, and GitHub author identity does not override Goddard ownership.

## Rationale

- **Matches the automation actor:** Background automation is owned by an authenticated developer, not by a repository subscription list.
- **Reduces client coordination:** SDK consumers and daemons maintain one stream instead of tracking repository-by-repository subscriptions.
- **Preserves isolation:** User-scoped routing prevents managed pull request feedback from leaking between Goddard users.
- **Keeps the transport simple:** Newline-delimited messages preserve the stable event model while remaining easy to consume across SDK, desktop, and daemon hosts.

## Consequences

- The backend must persist managed pull request ownership when a pull request is created so later feedback can be routed correctly.
- SDK, desktop, and daemon consumers subscribe once per authenticated user session rather than once per repository.
- Unmanaged pull requests are not delivered on the managed stream.
- Stream consumers treat delivery as an ordered sequence of typed messages rather than transport-specific event names.
- Delivery guarantees apply to managed pull requests whose ownership was recorded under this routing model; older records outside that guarantee boundary are not promised stream delivery.
