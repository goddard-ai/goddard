# Track C: Spec and Rollout

## Summary

The current canonical docs describe per-repository streaming, so the spec must move to a user-scoped managed-PR stream model. Rollout should be forward-only: newly created managed PRs are guaranteed to pair, while older records are not backfilled in this change.

## Spec Updates

- Update the daemon spec to say the runtime subscribes to one authenticated stream covering all managed PRs owned by the current Goddard user.
- Update core data-flow docs so webhook routing describes managed-PR ownership lookup before stream delivery.
- Update architecture docs and the relevant ADRs to replace repo-scoped SSE fan-out assumptions with user-scoped unified stream delivery.
- Record that GitHub PR author identity and Goddard managed-PR ownership are distinct concepts.

## Rollout Plan

- Ship backend persistence and routing before or together with client/daemon changes.
- Treat existing managed PR records as outside the guarantee boundary for this rollout.
- After deploy, validate with at least two users and multiple repos that stream isolation holds.
- Confirm unmanaged PR webhooks are ignored and do not leak onto any user stream.
- Remove repo-scoped subscription code in the same change; do not leave a fallback path.

## Verification Checklist

- Create a managed PR as user A and confirm its GitHub PR number is stored.
- Subscribe as user A and confirm events arrive from multiple repos over one stream.
- Subscribe as user B and confirm user A's events are not delivered.
- Trigger a webhook for an unmanaged PR and confirm no stream event is emitted.
- Trigger a `pr.created` event and confirm it lands on the creating user's stream.

## Assumptions

- The team will explicitly approve spec edits before implementation touches `spec/`.
- Forward-only rollout is acceptable for existing managed PRs.
- No deprecation notices, compatibility shims, or legacy repo-scoped fallbacks should be shipped.
