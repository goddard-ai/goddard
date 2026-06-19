# Auth Session

> The daemon keeps a local authentication session that daemon clients can read or clear. This page explains device-flow login, identity reads, logout, and why clients should not assume a separate auth state.

## Core idea

- The daemon owns the local auth session visible to daemon clients.
- Clients read or clear that state through the daemon rather than assuming a separate identity.

## Device flow

- Starting device auth creates a pending authentication flow.
- Completing the flow promotes a successful authentication into the current daemon-owned auth session.
- If the flow is abandoned or fails, clients should continue treating the previous daemon auth state as authoritative until a new success is recorded.

## Current identity

- Clients can ask the daemon who the current authenticated user is.
- The answer reflects the daemon's local auth session as-is.
- A missing identity is a normal state for unauthenticated local use, not proof that daemon records are unavailable.

## Logout

- Logout clears the daemon-owned auth session.
- Later operations that require auth need a new successful authentication flow.

## Boundaries

- Auth state is local daemon state.
- Reading auth state does not create sessions or start automation.
- Clearing auth state does not delete unrelated daemon records by itself.
