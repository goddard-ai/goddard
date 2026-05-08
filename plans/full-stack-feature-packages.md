# Full-Stack Feature Packages

Product ambiguity status: resolved for the high-level feature package intent.

## Intent

Organize Goddard product capabilities as internal full-stack feature packages so humans and AI agents can make product changes through clear, low-token extension points.

Each feature package should gather the layer-specific entrypoints for one product capability while preserving the existing layer boundaries:

- daemon entrypoint for runtime behavior and IPC registration
- SDK entrypoint for public SDK namespace contribution
- app entrypoint for UI composition
- schema entrypoint for validation and shared wire shapes when needed
- daemon IPC contract entrypoint for feature-owned daemon request and stream contracts when the feature has daemon/SDK communication
- backend entrypoint only when the feature has worker-hosted API behavior, persistence, webhooks, or real-time fan-out
- docs and glossary entries for local feature concepts
- tests scoped to the feature contracts

Public packages such as `@goddard-ai/sdk`, the daemon, and the app remain the supported composition roots. Feature packages are internal workspace packages and are not published as standalone public products.

Most feature packages are expected to live entirely in the daemon, SDK, and app layers. Backend entrypoints are optional and should be reserved for features that genuinely need backend authority.

## Package Shape

Feature packages live under the repository root `features/` directory and do not use a `feature-` package-name prefix.

The root workspace must include `features/*` before feature packages are scaffolded so package names resolve the same way as `core/*` and other internal workspaces.

Example:

```txt
features/
  session/
    package.json
    src/
      sdk.ts
      daemon.ts
      app.tsx
      schema.ts
      daemon-ipc.ts
```

Example package name:

```json
{
  "name": "@goddard-ai/session",
  "private": true
}
```

## Plugin Support Packages

Layer-specific plugin support packages provide the small contracts that feature entrypoints import. These packages should stay thin and should not become frameworks.

Examples:

- `@goddard-ai/sdk-plugin`
- `@goddard-ai/daemon-plugin`
- `@goddard-ai/app-plugin`

For the SDK layer, `@goddard-ai/sdk-plugin` should have close to zero runtime code. Its primary job is to expose types and a `defineSdkPlugin()` helper that exists for type inference.

All plugin support package `define*` helpers should use `const` type parameters so plugin values are exactly typed by what they define. This exact typing is required for composition roots to infer contributed namespaces, routes, slots, lifecycle participants, and other plugin metadata from the plugin list.

Feature packages import plugin support packages, not public composition roots:

```txt
@goddard-ai/sdk-plugin
  <- @goddard-ai/session
  <- @goddard-ai/sdk
```

This keeps the dependency graph acyclic while allowing `@goddard-ai/sdk` to bundle every feature package with an SDK entrypoint.

Plugin support packages are internal infrastructure packages. They may be imported by feature packages and public composition roots, but they are not the product-facing extension surface.

Plugin support packages should expose layer capability contracts and may re-export stable layer primitives when that prevents feature packages from importing broad internal packages. They should not become generic service locators or hide access to unrelated layer internals.

## Feature Package Dependency Rules

Feature packages should depend on plugin support packages and shared contract packages, not public composition roots.

Avoid package-level cycles such as:

```txt
@goddard-ai/sdk
  -> @goddard-ai/session
  -> @goddard-ai/sdk
```

When a feature app entrypoint needs SDK behavior, it should provide type-level SDK requirements that describe the SDK namespace or feature service shape it expects. The static app composition root already owns the composed SDK instance; composition should verify that the app context satisfies each feature's SDK requirements without the feature package importing `@goddard-ai/sdk`.

The same principle applies across layers: a feature package may import local feature entrypoints, shared schemas, daemon IPC contracts, and plugin support contracts, but it should not import the public package that will later bundle that same feature.

## Static Dependency Injection

Composition is static and dependency-injected.

Each public layer imports the relevant feature entrypoints and combines them into the supported product surface:

```ts
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"
import { workforceSdkPlugin } from "@goddard-ai/workforce/sdk"

export const defaultSdkPlugins = [sessionSdkPlugin, workforceSdkPlugin]
```

Feature entrypoints receive layer-owned services through explicit context objects selected by the static composition root. They should not import global clients, singleton SDK instances, daemon URLs, app state, or host-specific defaults.

Daemon feature interop uses first-class feature extensions. A daemon plugin can expose a named `provides` map and list other daemon plugin definitions in `consumes`; `setup(context)` receives the consumed feature extensions as direct `context.<feature>` fields, such as `context.session`. Methods and events share this feature-owned extension surface. Feature packages must not circularly depend on other feature packages.

## Feature Contributions And System Responsibilities

Feature packages contribute product-specific declarations, contracts, handlers, UI components, and feature-owned helpers.

Layer systems implement the generic substrate that composes, executes, secures, observes, and exposes those contributions:

- SDK system owns SDK construction, transport injection, namespace composition, public exports, and shared wrapper conventions.
- Daemon system owns IPC server mechanics, lifecycle orchestration, stream fan-out, persistence substrate, scheduling, diagnostics aggregation, and startup failure policy.
- App system owns shell layout, navigation placement, command routing, shortcut conflict semantics, desktop bridge boundaries, query-cache conventions, and design-system rules.
- Backend system owns Worker entrypoint composition, auth/session enforcement, database connection lifecycle, migration execution, webhook verification, SSE fan-out, scheduling, and diagnostics exposure.

Feature packages must not implement their own mini SDK runtime, daemon runtime, app shell, or backend platform. When a feature needs a shared mechanism, the layer's plugin support package or composition root should provide the capability through dependency injection.

## Shared Daemon IPC Contracts

Feature packages that expose daemon-backed SDK behavior should own a shared daemon IPC contract entrypoint, such as `src/daemon-ipc.ts`.

The daemon IPC contract defines transport-level names and validation contracts:

- daemon request names
- daemon request payload schemas
- daemon response schemas
- daemon stream names
- stream filter schemas
- stream payload schemas

The daemon IPC contract should import feature data shapes from `schema.ts` when shared schemas are needed. Keep `schema.ts` focused on data shapes and TypeScript types; keep `daemon-ipc.ts` focused on transport contracts.

Route contracts live with the feature; route handling lives in the daemon plugin; route calling lives in the SDK plugin.

Example:

```ts
export const sessionDaemonIpc = defineIpcSchema({
  requests: {
    "session.create": {
      payload: CreateSessionRequest,
      response: $type<CreateSessionResponse>(),
    },
  },
  streams: {
    "session.message": {
      filter: daemonSessionIdParamsSchema,
      payload: $type<SessionMessageEvent>(),
    },
  },
})
```

The public daemon and SDK composition roots should consume the same feature-owned daemon IPC contract so route names, stream names, request schemas, and response schemas do not drift across layers.

## Non-Goals

This is not a public plugin platform.

Do not add:

- runtime plugin discovery
- plugin manifests for third-party loading
- plugin install or enable/disable state
- dependency resolution between external plugins
- plugin permission systems
- third-party compatibility guarantees
- marketplace or publishing workflows

The goal is plugin-shaped internal modularity, not an external ecosystem.

## Benefits

- Product changes become easier to localize across daemon, SDK, app, schema, docs, and tests.
- SDK growth moves out of one central monolith and into feature-owned SDK entrypoints.
- Shared behavior parity becomes easier to review because a feature package can show which layers it contributes to.
- AI agents can inspect one feature package and the shared feature-package guidance before touching unrelated system areas.
- Internal scaffolding can make the correct structure faster than ad hoc file placement.

## Migration Sequence

Start with a low-risk feature that exercises daemon, SDK, and app entrypoints without backend behavior. This first migration should validate the feature package layout, static dependency injection, plugin support package shape, and public composition-root ergonomics without mixing in backend-specific authority or persistence concerns.

After the first pattern is working, migrate a backend-involved feature as the second reference case. That second migration should validate the optional backend entrypoint model and confirm that features without backend needs do not carry backend scaffolding by default.

## Scaffolding Tool

Add a small repository-local scaffolding tool for AI agents and humans to create new feature packages consistently.

The scaffold's default should create an inert internal feature package that is not part of any public product surface until a composition root imports and registers one of its entrypoints.

The scaffold should use `@clack/prompts` to ask which layers are needed before it writes files or updates package dependencies. Layer selection should drive generated entrypoints and common dependencies so app-only dependencies such as `@goddard-ai/styled-system` are added only when the selected feature layers need them.

The scaffold should create:

- `features/<name>/package.json`
- `features/<name>/tsconfig.json`
- `features/<name>/test/tsconfig.json`
- `features/<name>/tsdown.config.ts`
- `features/<name>/test/feature.test.ts`
- `features/<name>/src/sdk.ts` when SDK behavior is requested
- `features/<name>/src/daemon.ts` when daemon behavior is requested
- `features/<name>/src/app.tsx` when app UI is requested
- `features/<name>/src/app.style.ts` only when app styling is explicitly requested
- `features/<name>/src/schema.ts` when shared validation or wire shapes are requested
- `features/<name>/src/daemon-ipc.ts` when daemon and SDK entrypoints communicate through daemon IPC
- `features/<name>/src/backend.ts` only when backend behavior is requested
- placeholder tests that match the selected layer entrypoints

The scaffold should prefer explicit options over guessing. It should default to daemon, SDK, and app entrypoints for common local features, add a daemon IPC contract when both daemon and SDK entrypoints are selected, and add schema and backend entrypoints only when requested. It should not register the feature in public composition roots unless asked, because registration is the point where the feature becomes part of a supported product surface.

When a plugin support package re-exports stable primitives for its layer, the scaffold should prefer those imports over broad internal package imports. If a feature still needs a direct dependency such as `@goddard-ai/styled-system`, the scaffold should add it only for feature packages with an app entrypoint that actually imports it.

Use the scaffold from the workspace root:

```sh
bun run scaffold:feature
bun run scaffold:feature --name inbox --layers daemon,sdk,app --schema --daemon-ipc --dry-run
```

The implemented scaffold uses `@clack/prompts` for interactive selection and `cmd-ts` for noninteractive CLI parsing. It uses `radashi`'s `dedent` tagged template for generated file content.

## Reference Feature

`features/inbox` is the first daemon + SDK + app reference feature. It demonstrates:

- feature-owned schema and daemon IPC entrypoints
- SDK namespace contribution without importing `@goddard-ai/sdk`
- app contribution metadata without importing `@goddard-ai/sdk`
- daemon request handler contribution imported by the daemon composition root
- shared daemon IPC schema composition through `composeIpcSchemas()`

## Implementation Planning Questions

- How should feature package tests be split between layer-local tests and public composition-root tests?
- What daemon substrate API is needed before feature packages can own persistence-backed manager state such as the inbox manager and inbox item store?

These remaining questions are implementation-planning concerns rather than unresolved product ambiguity for the feature package direction.
