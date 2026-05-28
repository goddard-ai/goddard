# Daemon Feature Package Capabilities

Product ambiguity status: resolved.

## Purpose

Explore the daemon-level capabilities that feature packages are very likely to need when contributing local runtime behavior, IPC handlers, background work, and daemon-owned state.

This document is generic. It should guide the eventual shape of `@goddard-ai/daemon-plugin`, not define one feature's concrete daemon implementation.

## Likely Feature Inputs

Daemon feature entrypoints will likely need injected access to daemon-owned services:

- IPC route or handler registration
- feature-owned daemon IPC contract for request and stream validation
- stream publication and subscription coordination
- daemon-local persistence access when the feature owns local state
- resolved daemon configuration
- filesystem path resolvers for `.goddard` state
- logging and diagnostics hooks
- lifecycle hooks for daemon startup and shutdown
- background task scheduling owned by the daemon runtime
- access to authenticated backend clients where the daemon already owns that boundary
- process or agent-launch services for local automation features

Feature packages should not construct global daemon clients, read host environment directly, create independent config defaults, or bypass daemon-owned lifecycle management.

## Likely Feature Contributions

A daemon feature will usually contribute one or more of:

- IPC request handlers
- feature event producers for daemon-owned stream fan-out
- feature-owned daemon IPC contracts consumed by both daemon and SDK plugins
- daemon lifecycle participants
- background runtime definitions or handlers
- feature-owned repositories or store modules
- config resolvers or config consumers, when the feature owns daemon behavior
- diagnostics contributors
- health check contributors
- cleanup or migration declarations for feature-owned daemon-local data

Example shape:

```ts
export const sessionPlugin = definePlugin({
  name: "session",
  ipc: sessionDaemonIpc,
  register(context) {
    context.ipc.handle("session.create", createSession)
    context.streams.publish("session.message", publishSessionMessage)
  },
})
```

The daemon plugin should consume the feature's daemon IPC contract for route and stream registration. Handler implementation stays in the daemon plugin; transport names and validation contracts stay in the shared daemon IPC contract.

The daemon system, not individual features, owns IPC server mechanics, stream subscription tracking, stream filtering, fan-out delivery, lifecycle phase ordering, scheduler supervision, persistence substrate, migration execution, diagnostics aggregation, and startup failure policy. Feature packages contribute the domain-specific handlers and declarations that the daemon system executes.

## State And Defaults

Daemon features are likely to need clear boundaries around defaults and persistence:

- raw inputs should be normalized at named daemon boundaries
- behavior-affecting defaults should live in resolvers, not in handlers
- persisted feature state should be owned by explicit repositories or stores
- stream payloads should be derived from canonical feature state or daemon events
- diagnostics should expose enough context to debug without leaking host-only internals

If a feature affects shared behavior, its schema and SDK entrypoints should be updated in the same change.

## Composition Expectations

The daemon composition root imports all daemon feature entrypoints and registers them statically:

```ts
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { workforcePlugin } from "@goddard-ai/workforce/daemon"

composePlugins([sessionPlugin, workforcePlugin])
```

The daemon composition root should also compose feature-owned daemon IPC contracts into the daemon IPC schema that clients use. The SDK composition root should consume those same feature-owned contracts through the feature SDK plugins.

Registration should fail fast for duplicate IPC handlers, duplicate stream names, or incompatible lifecycle ownership.

Invalid daemon feature registration should block daemon startup in both development and packaged runtime. Registration conflicts are product-integrity failures; starting with a silently disabled or partially registered feature would make daemon behavior harder to trust.

## Non-Goals

- runtime loading of external daemon plugins
- feature-owned daemon process creation outside the daemon lifecycle
- direct app or SDK imports into daemon feature packages
- ad hoc persistence paths outside shared path resolvers
- feature-specific logging systems

## Implementation Planning Questions

- Should `definePlugin()` use one `const` type parameter for the full plugin object, or separate `const` parameters for name, IPC contract, lifecycle metadata, and registrations?
- Should daemon plugins declare the IPC names and stream names they own as metadata?
- Should daemon plugins provide migrations, or should migrations stay in a separate persistence layer?
- How should daemon plugins contribute to health checks without making health output noisy?
- What lifecycle phases are actually needed: configure, start, ready, shutdown?
