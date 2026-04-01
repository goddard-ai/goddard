# Webhook Integration Mismatch Review

- Scope:
  - This note covers the current mismatch between the exported GitHub App bridge and the backend webhook route contract.
  - It reflects the checked-out tree, not the pending `codex/ignore-closed-pr-events` branch.

## Summary

- The backend webhook route accepts only normalized `GitHubWebhookInput` payloads.
- The exported GitHub App bridge forwards raw GitHub webhook payloads from `app.webhooks.onAny(...)`.
- Those two shapes do not match.
- Result:
  - The normalized test path works.
  - The raw GitHub App forwarding path appears incomplete and likely fails route validation unless another normalizer exists outside this repository.

## Current Backend Contract

- The webhook route is `POST /webhooks/github`.
- Its request body schema is `GitHubWebhookInput`.
- `GitHubWebhookInput` currently accepts only:
  - `issue_comment`
  - `pull_request_review`
- The backend route does not branch on `x-github-event` headers.
- It passes `ctx.body` directly to `handleGitHubWebhook(...)`.

## Current GitHub App Bridge

- `createGitHubApp(...)` builds an Octokit `App` when app credentials are provided.
- `app.webhooks.onAny(...)` forwards the raw GitHub webhook `payload` to `POST /webhooks/github`.
- It includes `x-github-event` and `x-github-delivery` headers.
- It does not normalize the payload into the backend route's `GitHubWebhookInput` shape before sending it.

## Why This Is A Mismatch

- A raw GitHub `issue_comment.created` payload is not shaped like:
  - `{ type: "issue_comment", owner, repo, prNumber, author, body }`
- A raw GitHub `pull_request_review.submitted` payload is not shaped like:
  - `{ type: "pull_request_review", owner, repo, prNumber, author, state, body }`
- The backend route currently expects the normalized shape, not the raw GitHub payload shape plus event headers.
- I did not find any route-level normalization step in the backend that reads the forwarded headers and transforms the body.

## Evidence In Code

- Backend route contract:
  - `core/schema/src/backend/repo-events.ts`
  - `core/schema/src/backend/routes/webhooks.ts`
  - `core/backend/src/api/router.ts`
- GitHub App forwarding path:
  - `core/backend/src/github-app.ts`
- Tests that currently cover only the normalized helper path:
  - `core/backend/test/github-app.test.ts`
  - `core/backend/test/backend.test.ts`

## Behavioral Impact

- Human PR feedback routing is implemented only for normalized comment/review inputs.
- If production uses `createGitHubApp(...).app.webhooks.onAny(...)` as the ingress, reviewer feedback may never make it through `/webhooks/github`.
- The daemon-side feedback runtime depends on that backend event delivery, so the mismatch can block daemon notification entirely.
- `pull_request` events are also not part of the accepted webhook input contract today.
  - In `createGitHubApp(...)`, `pull_request` is only logged.

## Testing Gap

- Existing tests prove:
  - the backend can handle normalized webhook payloads
  - `handleWebhook(...)` can post normalized webhook payloads
- Existing tests do not prove:
  - raw GitHub webhook payloads forwarded by `app.webhooks.onAny(...)` are accepted by the backend route
  - the exported GitHub App bridge works end to end against the current route schema

## What Needs To Change

- Pick one authority boundary and make both sides match:
  - Option A:
    - Normalize in `createGitHubApp(...)` before POSTing to `/webhooks/github`.
  - Option B:
    - Change `/webhooks/github` to accept raw GitHub webhook payloads plus the event header, then normalize server-side.
- The current codebase already models normalized inputs cleanly.
- That makes Option A the smaller change unless there is a strong reason to move normalization into the backend.

## Related Gap

- Closed PR handling is separate from this mismatch.
- The current tree also lacks closed-PR eligibility filtering.
- The pending `codex/ignore-closed-pr-events` branch appears to address that separate problem.
