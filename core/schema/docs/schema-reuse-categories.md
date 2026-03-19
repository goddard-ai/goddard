# Schema Reuse Categories for `core/schema`

## Goal

Reduce duplicated Zod contract definitions across HTTP routes, daemon IPC, and shared SDK-facing modules, while also breaking route modules into smaller feature-oriented files without introducing `index.ts` umbrellas.

## Current Findings

### 1. `src/backend.ts` and `src/types.ts` are duplicate sources of truth

- `src/backend.ts` and `src/types.ts` are currently identical.
- That makes every backend-facing contract easy to drift, even before route reuse is considered.
- Recommendation: treat one umbrella module as canonical and move real definitions into smaller category modules that the umbrella re-exports.

### 2. Backend route request schemas are re-declared instead of reused

`src/backend/routes.ts` repeats shapes that already exist in the backend schema layer:

- device flow payloads
- auth session header shape
- repo ref fields
- PR create/reply inputs
- GitHub webhook discriminated unions

The route file is therefore both an umbrella module and a second schema definition site.

### 3. Daemon route and IPC payload schemas repeat the same session and PR concepts

`src/daemon/routes.ts` and `src/daemon-ipc.ts` both define local versions of:

- authorization header shapes
- PR submit / reply request bodies
- session id path or payload objects
- agent distribution schemas
- session creation payloads
- daemon session metadata fragments

This is the largest current reuse gap across transport boundaries.

### 4. Route modules are organized by transport, not by feature slice

The two route umbrella files each mix unrelated areas:

- `src/backend/routes.ts`: auth, PRs, webhooks, stream
- `src/daemon/routes.ts`: health, PRs, sessions

That makes the module boundary monolithic even when the underlying contracts are already feature-specific.

### 5. `zod` vs `zod/mini` is a practical reuse boundary today

The shared schema modules use `zod`, while route modules use `zod/mini`. Even where the contracts are conceptually shared, the code currently re-states them instead of importing common leaves.

Recommendation: create small transport-neutral category modules and choose one deliberate reuse pattern per category:

- canonical leaf schemas that both route and non-route modules can import directly, or
- canonical field fragments / builders that let `zod` and `zod/mini` compose the same contract without repeating literals

The important part is that each concept has one canonical definition site.

### 6. Upgrading `rouzer` to 2.0.0 would remove the current schema split

`rouzer` 2.0.0 uses `zod` instead of `zod/mini`.

That upgrade would materially improve this refactor because it would:

- let route modules import the same canonical Zod schemas used by non-route modules
- reduce the need for duplicate route-local field definitions and schema builders
- make transport-neutral leaf modules much simpler, because they would no longer need to account for two Zod variants

Recommendation:

- treat the `rouzer` 2.0.0 upgrade as an enabling step for schema reuse work
- prefer doing the upgrade before or alongside route modularization
- after the upgrade, standardize on `zod` for both shared schema modules and route modules

Without that upgrade, schema reuse is still possible, but more of the implementation would need fragment-level composition instead of straightforward schema imports.

## Recommended Reuse Categories

These categories are the ones most likely to pay off across backend routes, daemon routes, daemon IPC, and SDK consumers.

### 1. Auth and Session Identity

Owns:

- bearer authorization headers
- GitHub device flow start / complete payloads
- authenticated backend session payloads
- daemon session id params / payloads
- token-to-session resolution inputs

Suggested modules:

- `src/common/auth.ts`
- `src/backend/auth.ts`
- `src/daemon/session-identity.ts`

Why:

- auth header shapes already repeat between backend and daemon routes
- device flow contracts belong together instead of living beside PR and webhook contracts
- session id payloads repeat across daemon routes and daemon IPC

### 2. Repository Addressing and Pull Request Inputs

Owns:

- repo refs
- PR create inputs
- PR reply inputs
- managed-PR lookup query shapes
- daemon PR submit / reply inputs
- PR record payloads

Suggested modules:

- `src/common/repository.ts`
- `src/backend/pull-requests.ts`
- `src/daemon/pull-requests.ts`

Why:

- `owner`, `repo`, and `prNumber` fields recur throughout backend contracts
- daemon PR contracts are transport-specific, but they still share a clear category with backend PR concerns

### 3. GitHub Feedback and Event Normalization

Owns:

- webhook input schemas
- normalized repo event schemas
- stream message schemas

Suggested modules:

- `src/backend/repo-events.ts`
- `src/backend/webhooks.ts`

Why:

- webhook payloads and normalized stream events are tightly related but currently embedded inside broader backend modules
- this split clarifies the boundary between incoming GitHub transport data and internal normalized event contracts

### 4. Agent Distribution and Launch Configuration

Owns:

- binary target schema
- binary distribution schema
- package distribution schema
- agent distribution schema
- shared install-method validation

Suggested modules:

- `src/daemon/agent-distribution.ts`
- `src/session-server/agent-distribution.ts`

Why:

- this schema family is duplicated almost verbatim in `src/daemon/routes.ts` and `src/daemon-ipc.ts`
- it is also logically shared with `src/session-server.ts`

### 5. Daemon Session Creation and Metadata

Owns:

- daemon session metadata
- create session request payload
- session connection / diagnostics-related request fragments
- initial prompt and one-shot input variants

Suggested modules:

- `src/daemon/session-metadata.ts`
- `src/daemon/sessions.ts`

Why:

- session creation is a cross-transport contract used by daemon routes, daemon IPC, and SDK-facing daemon types
- it should not be rebuilt independently in each boundary module

### 6. Workforce Command Fragments

Owns:

- `rootDir`
- `requestId`
- `targetAgentId`
- `token`
- `intent`
- shared mutate-response payloads

Suggested modules:

- `src/workforce/requests.ts`
- `src/daemon/workforce.ts`

Why:

- workforce currently lives mostly in type-only modules, but its IPC request shapes are already repeating common fragments
- creating a category now would prevent future daemon route growth from copying those literals again

### 7. Shared Primitive Field Sets

Owns:

- `id` path params
- timestamp strings
- URL strings
- non-empty named resources
- common metadata records

Suggested modules:

- `src/common/primitives.ts`
- `src/common/params.ts`

Why:

- small primitives are where route duplication starts
- once these are canonical, larger route and IPC contracts become composition instead of repetition

## Recommended Route Modularization

Keep the current umbrella module names and split the actual route declarations into feature modules beneath them.

### Backend routes

Recommended structure:

```text
src/backend/routes.ts
src/backend/routes/auth.ts
src/backend/routes/pull-requests.ts
src/backend/routes/webhooks.ts
src/backend/routes/stream.ts
```

Recommended responsibility split:

- `src/backend/routes/auth.ts`
  - `authDeviceStartRoute`
  - `authDeviceCompleteRoute`
  - `authSessionRoute`
- `src/backend/routes/pull-requests.ts`
  - `prCreateRoute`
  - `prReplyRoute`
  - `prManagedRoute`
- `src/backend/routes/webhooks.ts`
  - `githubWebhookRoute`
- `src/backend/routes/stream.ts`
  - `repoStreamRoute`
- `src/backend/routes.ts`
  - umbrella re-exports only

### Daemon routes

Recommended structure:

```text
src/daemon/routes.ts
src/daemon/routes/health.ts
src/daemon/routes/pull-requests.ts
src/daemon/routes/sessions.ts
```

Recommended responsibility split:

- `src/daemon/routes/health.ts`
  - `healthRoute`
- `src/daemon/routes/pull-requests.ts`
  - `prSubmitRoute`
  - `prReplyRoute`
- `src/daemon/routes/sessions.ts`
  - `sessionCreateRoute`
  - `sessionGetRoute`
  - `sessionHistoryRoute`
  - `sessionShutdownRoute`
- `src/daemon/routes.ts`
  - umbrella re-exports only

If daemon workforce routes are added later, prefer a dedicated `src/daemon/routes/workforce.ts` instead of growing `src/daemon/routes.ts`.

## Suggested Target Layout

This is a concrete direction that keeps umbrella modules named after the domain rather than `index.ts`.

```text
src/common/auth.ts
src/common/params.ts
src/common/primitives.ts
src/common/repository.ts

src/backend.ts
src/backend/auth.ts
src/backend/pull-requests.ts
src/backend/repo-events.ts
src/backend/webhooks.ts
src/backend/routes.ts
src/backend/routes/auth.ts
src/backend/routes/pull-requests.ts
src/backend/routes/webhooks.ts
src/backend/routes/stream.ts

src/daemon.ts
src/daemon/agent-distribution.ts
src/daemon/pull-requests.ts
src/daemon/session-identity.ts
src/daemon/session-metadata.ts
src/daemon/sessions.ts
src/daemon/workforce.ts
src/daemon/routes.ts
src/daemon/routes/health.ts
src/daemon/routes/pull-requests.ts
src/daemon/routes/sessions.ts
```

## Prioritization

If this is done incrementally, the highest-value order is:

1. Upgrade `rouzer` to 2.0.0 so route modules can use `zod` directly instead of `zod/mini`.
2. Eliminate the `src/backend.ts` / `src/types.ts` duplication by moving backend contracts into category modules and keeping one umbrella.
3. Extract auth, repo, and PR leaf schemas because they are reused across several backend route contracts.
4. Extract daemon agent distribution and session creation leaf schemas because they are duplicated across routes and IPC.
5. Split `src/backend/routes.ts` and `src/daemon/routes.ts` into feature submodules once shared leaves exist.
6. Normalize workforce request fragments before any new workforce transport boundary is added.

## Short Version

The categories most worth creating are:

- auth and session identity
- repository and pull request inputs
- GitHub feedback and normalized repo events
- agent distribution and launch configuration
- daemon session creation and metadata
- workforce command fragments
- shared primitive field sets

The route modularization should follow the same feature slices, with `src/backend/routes.ts` and `src/daemon/routes.ts` kept as umbrella modules that only re-export submodules instead of defining every route inline. Upgrading `rouzer` to 2.0.0 first would make that refactor cleaner by removing the current `zod` / `zod/mini` split.
