# Full-Stack Feature Packages

## Intent

Organize Goddard product capabilities as internal full-stack feature packages so humans and AI agents can make product changes through clear, low-token extension points.

Each feature package should gather the layer-specific entrypoints for one product capability while preserving the existing layer boundaries:

- daemon entrypoint for runtime behavior and IPC registration
- SDK entrypoint for public SDK namespace contribution
- app entrypoint for UI composition
- schema entrypoint for validation and shared wire shapes when needed
- docs and glossary entries for local feature concepts
- tests scoped to the feature contracts

Public packages such as `@goddard-ai/sdk`, the daemon, and the app remain the supported composition roots. Feature packages are internal workspace packages and are not published as standalone public products.

## Package Shape

Feature packages live under the repository root `features/` directory and do not use a `feature-` package-name prefix.

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

## Static Dependency Injection

Composition is static and dependency-injected.

Each public layer imports the relevant feature entrypoints and combines them into the supported product surface:

```ts
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"
import { workforceSdkPlugin } from "@goddard-ai/workforce/sdk"

export const defaultSdkPlugins = [sessionSdkPlugin, workforceSdkPlugin]
```

Feature entrypoints receive layer-owned services through explicit context objects. They should not import global clients, singleton SDK instances, daemon URLs, app state, or host-specific defaults.

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

## Scaffolding Tool

Add a small repository-local scaffolding tool for AI agents and humans to create new feature packages consistently.

The scaffold should create:

- `features/<name>/package.json`
- `features/<name>/README.md`
- `features/<name>/glossary.md` when the feature introduces domain terminology
- `features/<name>/src/sdk.ts` when SDK behavior is requested
- `features/<name>/src/daemon.ts` when daemon behavior is requested
- `features/<name>/src/app.tsx` when app UI is requested
- `features/<name>/src/schema.ts` when shared validation or wire shapes are requested
- placeholder tests that match the selected layer entrypoints

The scaffold should prefer explicit options over guessing. It should not register the feature in public composition roots unless asked, because registration is the point where the feature becomes part of a supported product surface.

## Open Questions

- Which feature should be migrated first as the reference implementation?
- Should plugin support packages live under `core/`, `packages/`, or another root-level workspace folder?
- What is the minimum SDK plugin contract needed to preserve the current `new GoddardSdk({ client })` ergonomics?
- How should feature package tests be split between layer-local tests and public composition-root tests?
- Should feature package READMEs follow a required template?
