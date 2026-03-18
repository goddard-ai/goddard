# Unified Daemon Stream Subscription

## Summary

This plan replaces repo-scoped daemon subscriptions with a single authenticated user-scoped stream. The backend will persist the Goddard user who initiated each managed PR at creation time, then use that ownership record plus the real GitHub PR number to route webhook events back to the correct unified stream subscriber.

This plan assumes the safest choices:

- No deprecation period or legacy fallback path.
- Forward-only rollout for newly created managed PRs.
- Production Cloudflare routing is specified now rather than deferred.

## Parallel Workstreams

### Track A: Backend Data and Stream Routing

Owner: backend/platform

Doc: [01-backend-data-and-routing.md](./01-backend-data-and-routing.md)

Deliverables:

- Persist real GitHub PR identity and Goddard ownership on PR creation.
- Replace repo-scoped `/stream` semantics with authenticated user-scoped semantics.
- Route webhook and PR-created events through user-scoped stream fan-out.
- Define the production Worker topology for unified streams.

### Track B: Public Clients and Daemon

Owner: SDK/runtime

Doc: [02-client-and-daemon.md](./02-client-and-daemon.md)

Deliverables:

- Replace `subscribeToRepo(...)` with `subscribe()` in `@goddard-ai/backend/client`.
- Replace `subscribeToRepo(...)` with `subscribe()` in `@goddard-ai/sdk`.
- Update the daemon to establish one stream subscription per process.

Dependency:

- Can start immediately on API shape and tests.
- Final verification depends on Track A landing the new `/stream` contract.

### Track C: Spec and Rollout

Owner: architecture/docs

Doc: [03-spec-and-rollout.md](./03-spec-and-rollout.md)

Deliverables:

- Update spec language from repo-scoped to user-scoped stream ownership.
- Record the production architecture decision.
- Capture the forward-only rollout and post-deploy validation steps.

Dependency:

- Should be reviewed alongside Track A because it changes canonical platform behavior.

## Critical Decisions Locked

- `PullRequestRecord.createdBy` continues to mean the Goddard user who initiated and owns the managed PR, not the GitHub author account.
- Webhook pairing is done via backend-managed PR records, not GitHub author identity.
- Existing managed PRs are not backfilled for this rollout.
- SSE payloads remain `{ event: RepoEvent }`.
- `/stream` keeps the same path but no longer accepts `owner` and `repo` query parameters.

## Acceptance Criteria

- A signed-in daemon opens one SSE connection and receives events for all managed PRs owned by that user across repos.
- Webhook events for unmanaged PRs are not emitted on the unified stream.
- Webhook events for another Goddard user's PRs are not emitted on the current user's stream.
- PR creation persists enough metadata to pair later webhooks without relying on GitHub PR author identity.
- There is no legacy repo-scoped subscription code path left behind in clients or daemon.
