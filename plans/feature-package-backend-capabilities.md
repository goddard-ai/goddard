# Backend Feature Package Capabilities

Product ambiguity status: resolved.

## Purpose

Explore the backend-level capabilities that feature packages are very likely to need when contributing worker-hosted API behavior, persistence, webhooks, and real-time fan-out.

This document is generic. It should guide the eventual shape of `@goddard-ai/backend-plugin`, not define one feature's concrete backend behavior.

Backend entrypoints are optional. Most Goddard feature packages are expected to work through daemon, SDK, and app entrypoints only; backend plugins are for features that need backend-owned authority, persistence, webhook handling, or real-time fan-out.

## Likely Feature Inputs

Backend feature entrypoints will likely need injected access to backend-owned services:

- route registration
- authenticated session context
- database access through the backend's persistence abstraction
- transaction helpers when available
- schema validation helpers
- event publication for SSE or other real-time fan-out
- GitHub App or OAuth clients where the backend owns delegated identity
- webhook verification and dispatch services
- secret and environment bindings exposed through typed backend config
- logging and request diagnostics

Feature packages should not read raw worker environment bindings directly unless the backend plugin context explicitly owns that boundary. They should not construct independent database clients, bypass auth middleware, or publish events outside backend-owned fan-out services.

## Likely Feature Contributions

A backend feature will usually contribute one or more of:

- authenticated API routes
- unauthenticated callback or webhook routes
- webhook event handlers
- database table declarations or repository modules
- backend event producers
- SSE topic or event definitions
- auth policy requirements or policy checks through injected helpers
- backend-owned integrations with GitHub or other external services
- scheduled task handlers if the platform supports them
- diagnostics or health contributors

Example shape:

```ts
export const pullRequestBackendPlugin = defineBackendPlugin({
  name: "pull-request",
  register(context) {
    context.routes.post("/api/pr/submit", submitPullRequest)
    context.webhooks.github.on("pull_request_review_comment", handleReviewComment)
  },
})
```

The backend system, not individual features, owns Worker entrypoint composition, auth/session enforcement, database connection lifecycle, transaction boundaries, migration execution, webhook verification, SSE fan-out, scheduler execution, and diagnostics exposure. Feature packages contribute route handlers, webhook handlers, event producers, persistence declarations, repository modules, policy requirements, and scheduled task handlers that the backend system runs.

## Data And Authority Rules

Backend features should preserve Goddard's authority boundaries:

- backend routes should validate inputs at the boundary
- database writes should go through feature-owned repositories or explicit persistence helpers
- delegated GitHub identity should stay behind backend-owned auth and integration services
- SSE fan-out should follow authenticated ownership rules
- feature-owned database shapes should be reflected in schema and migrations when needed
- daemon and SDK behavior should consume backend contracts rather than duplicating backend authority

## Composition Expectations

The backend composition root imports all backend feature entrypoints that belong in the Cloudflare Worker product:

```ts
import { authBackendPlugin } from "@goddard-ai/auth/backend"
import { pullRequestBackendPlugin } from "@goddard-ai/pull-request/backend"

registerBackendPlugins([authBackendPlugin, pullRequestBackendPlugin])
```

Registration should fail fast for duplicate routes, duplicate webhook handlers where exclusivity is required, or conflicting event names.

Multiple backend features may handle the same webhook event. Webhook dispatch should use deterministic ordering and isolate handler failures so one feature's failure does not silently prevent other interested features from receiving the same platform signal.

## Non-Goals

- runtime loading of external backend plugins
- feature-owned Worker entrypoints
- raw environment access from feature packages by default
- feature-owned database clients
- bypassing shared auth/session validation
- backend APIs that are not reflected in shared schema where clients depend on them

## Implementation Planning Questions

- Should `defineBackendPlugin()` use one `const` type parameter for the full plugin object, or separate `const` parameters for routes, webhooks, events, and scheduled tasks?
- Should backend plugins own database table definitions, repository modules, or both?
- How should backend plugins declare migrations without coupling feature package layout to one migration tool?
- Should SSE event names be globally registered through backend plugins or schema plugins?
