# SDK Feature Package Capabilities

## Purpose

Explore the SDK-level capabilities that feature packages are very likely to need when contributing to Goddard's public SDK surface.

This document is generic. It should guide the eventual shape of `@goddard-ai/sdk-plugin`, not define one feature's concrete API.

## Likely Feature Inputs

SDK feature entrypoints will likely need a small injected context:

- daemon IPC client for request/response calls
- daemon stream subscription function for live updates
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
- stream helpers that apply daemon-side filters and unwrap payloads
- object-backed wrappers for long-lived daemon resources
- SDK-owned convenience message builders when raw protocol frames should stay hidden
- type exports for feature-specific request and response shapes
- feature-local helper exports that are stable enough for SDK consumers

## Likely Type Needs

The SDK plugin support package is mostly a type-inference boundary. It likely needs:

- `defineSdkPlugin()`
- `SdkPlugin`
- `SdkPluginContext`
- namespace-name typing
- namespace-surface typing
- duplicate namespace detection for default composition if TypeScript can support it cleanly

The support package should avoid becoming a runtime framework. Runtime composition can live in `@goddard-ai/sdk` if that keeps the plugin package thinner.

## Composition Expectations

The public SDK package imports all feature SDK entrypoints that belong in the supported SDK bundle:

```ts
import { sessionSdkPlugin } from "@goddard-ai/session/sdk"
import { workforceSdkPlugin } from "@goddard-ai/workforce/sdk"

export const defaultSdkPlugins = [sessionSdkPlugin, workforceSdkPlugin]
```

The default `GoddardSdk` constructor should preserve the current user-facing ergonomics:

```ts
const sdk = new GoddardSdk({ client })
await sdk.session.create(...)
```

Custom SDK construction can exist for internal tests or specialized hosts, but it should not imply a public plugin ecosystem.

## Non-Goals

- runtime plugin discovery
- feature package publication as standalone public SDK plugins
- host-specific transport setup inside feature packages
- app-only state helpers inside SDK features
- daemon behavior implemented in the SDK layer

## Open Questions

- Should SDK feature plugins return a namespace object, or should they receive a registry and call `registerNamespace()`?
- Should stream helpers be part of the generic SDK context or just use the daemon client directly?
- Should object-backed wrappers be feature-owned, SDK-kernel-owned, or split by feature?
- Should the SDK expose optional subpath imports for individual feature namespaces?
