# Track A: Backend Data and Stream Routing

## Summary

The backend must stop inferring managed PR ownership from GitHub authorship and instead persist the Goddard owner at PR creation time. Unified stream routing then becomes: resolve managed PR owner from backend state, publish the event to that user's stream, and let the daemon consume one authenticated SSE connection.

## Implementation Changes

### Managed PR Persistence

- Update PR creation so the stored PR record contains the real GitHub PR number returned by the GitHub App flow.
- Persist the Goddard user who initiated the PR as the managed owner at creation time.
- Keep `createdBy` as the public Goddard-owner field for now, but document that it is not the GitHub author account.
- Ensure managed PR lookup is keyed by `owner + repo + prNumber`.
- Add any schema migration and index support needed so webhook lookup by repo and PR number is deterministic and cheap.

### Backend Contract

- Keep `GET /stream` on the same path.
- Remove repo query parameters from the route contract.
- Authenticate the request, resolve the Goddard user from the session, and attach the connection to that user's unified stream.
- Keep SSE frame payloads unchanged as `{ event: RepoEvent }`.

### Event Routing Logic

- Route `pr.created` events to the owner who initiated the PR through Goddard.
- Route webhook events only when the referenced PR exists in managed PR storage.
- Resolve the owning Goddard user from the managed PR record and publish the event only to that user stream.
- Do not emit webhook events for unmanaged PRs or missing PR records.

### In-Memory Development Path

- Replace stream sinks keyed by repo with stream sinks keyed by GitHub username.
- On broadcast, resolve the managed PR owner first, then send to that user's stream set.
- Mirror production semantics closely so integration tests exercise the same routing model.

### Production Cloudflare Topology

- Replace the repo-scoped stream assumption with user-scoped stream delivery.
- Use the request/webhook handler to resolve the managed PR owner from durable storage.
- Publish the resulting `RepoEvent` to a user-scoped stream channel keyed by the Goddard user.
- Attach SSE subscribers to that user-scoped channel, not to repo-scoped fan-out state.
- Do not keep a compatibility layer that multiplexes the new contract through repo subscriptions.

## Public Interfaces

- `GET /stream` becomes authenticated and user-scoped.
- `repoStreamRoute` no longer requires `owner` or `repo`.
- `RepoEvent` wire shape is unchanged.

## Test Plan

- Route/schema tests verify `/stream` path stability and removal of repo query parsing.
- Backend integration tests verify one user receives events for managed PRs across multiple repos over one stream.
- Backend integration tests verify a second user does not receive those events.
- Backend integration tests verify unmanaged PR webhooks do not produce unified stream events.
- Persistence tests verify the stored PR record uses the GitHub PR number and the Goddard owner recorded at creation time.

## Risks to Watch

- If PR creation still stores synthetic PR numbers, webhook pairing will fail even if user-scoped streaming is correct.
- If production fan-out still assumes repo ownership, the local implementation will pass tests while the deployed system remains incompatible.
