# Daemon Feature Package Capabilities

## Purpose

Explore the daemon-level capabilities that feature packages are very likely to need when contributing local runtime behavior, IPC handlers, background work, and daemon-owned state.

This document is generic. It should guide the eventual shape of `@goddard-ai/daemon-plugin`, not define one feature's concrete daemon implementation.

## Likely Feature Inputs

Daemon feature entrypoints will likely need injected access to daemon-owned services:

- IPC route or handler registration
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
- IPC stream publishers
- daemon startup hooks
- daemon shutdown hooks
- background runtime registrations
- local persistence stores or repositories
- config resolvers or config consumers, when the feature owns daemon behavior
- diagnostics providers
- health check contributors
- cleanup or migration hooks for feature-owned daemon-local data

Example shape:

```ts
export const sessionDaemonPlugin = defineDaemonPlugin({
  name: "session",
  register(context) {
    context.ipc.handle("session.create", createSession)
    context.streams.publish("session.message", publishSessionMessage)
  },
})
```

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
import { sessionDaemonPlugin } from "@goddard-ai/session/daemon"
import { workforceDaemonPlugin } from "@goddard-ai/workforce/daemon"

registerDaemonPlugins([sessionDaemonPlugin, workforceDaemonPlugin])
```

Registration should fail fast for duplicate IPC handlers, duplicate stream names, or incompatible lifecycle ownership.

## Non-Goals

- runtime loading of external daemon plugins
- feature-owned daemon process creation outside the daemon lifecycle
- direct app or SDK imports into daemon feature packages
- ad hoc persistence paths outside shared path resolvers
- feature-specific logging systems

## Open Questions

- Should daemon plugins declare the IPC names and stream names they own as metadata?
- Should daemon plugins provide migrations, or should migrations stay in a separate persistence layer?
- How should daemon plugins contribute to health checks without making health output noisy?
- What lifecycle phases are actually needed: configure, start, ready, shutdown?
