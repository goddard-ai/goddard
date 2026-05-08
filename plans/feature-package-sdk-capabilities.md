# SDK Feature Package Capabilities

Product ambiguity status: resolved.

## Purpose

Explore the SDK-level capabilities that feature packages are very likely to need when contributing to Goddard's public SDK surface.

This document is generic. It should guide the eventual shape of `@goddard-ai/sdk-plugin`, not define one feature's concrete API.

## Likely Feature Inputs

SDK feature entrypoints will likely need a small injected context:

- daemon IPC client for request/response calls
- daemon stream subscription function for live updates
- feature-owned daemon IPC contract for route names, stream names, and payload types
- namespace registration utility or return contract
- shared SDK error normalization when the SDK owns any error shape
- optional access to SDK-owned helper factories for long-lived wrappers

The context should not include host-specific daemon URL resolution, Node environment loading, browser globals, app state, or singleton SDK instances.

## Likely Feature Contributions

An SDK feature will usually contribute one public namespace:

```ts
export const sessionSdkPlugin = defineSdkPlugin({
  namespace: "session",
  create(context) {
    return {
      create: (input) => context.client.send("session.create", input),
    }
  },
})
```

Likely contribution types:

- thin daemon IPC method wrappers
- methods generated from or checked against feature-owned daemon IPC contracts
- stream helpers that apply daemon-side filters and unwrap payloads
- feature-specific wrapper factories for long-lived daemon resources
- feature-specific convenience builders when raw protocol frames should stay hidden
- internal type exports for feature-specific request and response shapes
- feature-local helper exports that the composed SDK may choose to expose publicly

SDK plugins that call daemon-backed behavior should consume the feature's shared daemon IPC contract instead of duplicating route or stream names locally. The SDK plugin owns the user-facing namespace shape; the daemon IPC contract owns the transport names and validation shapes.

The SDK system, not individual features, owns SDK construction, transport injection, namespace composition, public export policy, and shared wrapper conventions. Feature packages can contribute namespace surfaces and feature-specific helpers, but the composed `@goddard-ai/sdk` package decides which helpers and types become public SDK API.

## Likely Type Needs

The SDK plugin support package is mostly a type-inference boundary. It likely needs:

- `defineSdkPlugin()`
- `SdkPlugin`
- `SdkPluginContext`
- namespace-name typing
- namespace-surface typing
- duplicate namespace detection for default composition if TypeScript can support it cleanly

`defineSdkPlugin()` should use `const` type parameters so the plugin value preserves the exact namespace, method surface, and metadata needed for composition-time type inference.

The support package should avoid becoming a runtime framework. Runtime composition can live in `@goddard-ai/sdk` if that keeps the plugin package thinner.

## Composition Expectations

The public SDK package imports all feature SDK entrypoints that belong in the supported SDK bundle:

```ts
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"
import { workforceSdkPlugin } from "@goddard-ai/workforce/sdk"

export const defaultSdkPlugins = [sessionSdkPlugin, workforceSdkPlugin]
```

The public SDK composition root should compose against the same feature-owned daemon IPC contracts that the daemon composition root uses. This keeps `@goddard-ai/sdk` as the supported public surface while avoiding drift between SDK method wiring and daemon IPC registration.

The default `GoddardSdk` constructor should preserve the current user-facing ergonomics:

```ts
const sdk = new GoddardSdk({ client })
await sdk.session.create(...)
```

Custom SDK plugin composition is internal-only for now. It can be used by Goddard's own SDK bundle and tests, but it is not a supported public API for SDK consumers. Public consumers should rely on composed SDK entrypoints such as `new GoddardSdk({ client })`, not on assembling feature plugins themselves.

The SDK plugin support package should remain internal infrastructure. It can provide type inference and shared plugin contracts for feature packages, but it should not imply third-party SDK plugin support.

Individual feature SDK namespace subpaths should remain internal for now. The public SDK surface is the composed `@goddard-ai/sdk` and `@goddard-ai/sdk/node` entrypoints, not per-feature imports such as `@goddard-ai/sdk/session`.

## Non-Goals

- runtime plugin discovery
- feature package publication as standalone public SDK plugins
- public per-feature SDK namespace imports
- host-specific transport setup inside feature packages
- app-only state helpers inside SDK features
- daemon behavior implemented in the SDK layer

## Implementation Planning Questions

- Should SDK feature plugins return a namespace object, or should they receive a registry and call `registerNamespace()`?
- Should stream helpers be part of the generic SDK context or just use the daemon client directly?
- Should object-backed wrappers be feature-owned, SDK-kernel-owned, or split by feature?
