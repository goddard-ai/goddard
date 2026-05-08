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
    README.md
    glossary.md
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

Feature packages import plugin support packages, not public composition roots:

```txt
@goddard-ai/sdk-plugin
  <- @goddard-ai/session
  <- @goddard-ai/sdk
```

This keeps the dependency graph acyclic while allowing `@goddard-ai/sdk` to bundle every feature package with an SDK entrypoint.

Plugin support packages are internal infrastructure packages. They may be imported by feature packages and public composition roots, but they are not the product-facing extension surface.

## Static Dependency Injection

Composition is static and dependency-injected.

Each public layer imports the relevant feature entrypoints and combines them into the supported product surface:

```ts
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"
import { workforceSdkPlugin } from "@goddard-ai/workforce/sdk"

export const defaultSdkPlugins = [sessionSdkPlugin, workforceSdkPlugin]
```

Feature entrypoints receive layer-owned services through explicit context objects. They should not import global clients, singleton SDK instances, daemon URLs, app state, or host-specific defaults.

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
export const sessionDaemonIpc = defineDaemonIpcContract({
  requests: {
    "session.create": {
      input: createSessionRequestSchema,
      output: createSessionResponseSchema,
    },
  },
  streams: {
    "session.message": {
      filter: daemonSessionIdParamsSchema,
      payload: sessionMessageEnvelopeSchema,
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
- AI agents can inspect one feature package and its README before touching unrelated system areas.
- Internal scaffolding can make the correct structure faster than ad hoc file placement.

## Migration Sequence

Start with a low-risk feature that exercises daemon, SDK, and app entrypoints without backend behavior. This first migration should validate the feature package layout, static dependency injection, plugin support package shape, and public composition-root ergonomics without mixing in backend-specific authority or persistence concerns.

After the first pattern is working, migrate a backend-involved feature as the second reference case. That second migration should validate the optional backend entrypoint model and confirm that features without backend needs do not carry backend scaffolding by default.

## Scaffolding Tool

Add a small repository-local scaffolding tool for AI agents and humans to create new feature packages consistently.

The scaffold's default should create an inert internal feature package that is not part of any public product surface until a composition root imports and registers one of its entrypoints.

The scaffold should create:

- `features/<name>/package.json`
- `features/<name>/README.md`
- `features/<name>/glossary.md` when the feature introduces domain terminology
- `features/<name>/src/sdk.ts` when SDK behavior is requested
- `features/<name>/src/daemon.ts` when daemon behavior is requested
- `features/<name>/src/app.tsx` when app UI is requested
- `features/<name>/src/schema.ts` when shared validation or wire shapes are requested
- `features/<name>/src/daemon-ipc.ts` when daemon and SDK entrypoints communicate through daemon IPC
- `features/<name>/src/backend.ts` only when backend behavior is requested
- placeholder tests that match the selected layer entrypoints

The scaffold should prefer explicit options over guessing. It should default to daemon, SDK, and app entrypoints for common local features, add a daemon IPC contract when both daemon and SDK entrypoints are selected, and add schema and backend entrypoints only when requested. It should not register the feature in public composition roots unless asked, because registration is the point where the feature becomes part of a supported product surface.

## Feature README Template

Each feature package README should be short and consistent so agents can recover the feature's purpose without reading the whole implementation.

Required sections:

- `Purpose`: the product capability the feature owns
- `Entrypoints`: which stack layers the feature contributes to
- `Composition`: which public composition roots import the feature
- `Boundaries`: what the feature owns and must not bypass
- `IPC Contracts`: daemon request and stream contracts the feature owns, when applicable
- `Related Docs`: nearby glossary, spec, or concept documents when they exist

## Implementation Planning Questions

- What is the minimum SDK plugin contract needed to preserve the current `new GoddardSdk({ client })` ergonomics?
- How should feature package tests be split between layer-local tests and public composition-root tests?

These remaining questions are implementation-planning concerns rather than unresolved product ambiguity for the feature package direction.
