# `@goddard-ai/daemon-plugin`

Internal daemon plugin support contracts for feature packages.

This package is infrastructure for statically composed Goddard feature packages. It is not a public plugin platform; its runtime surface is limited to deterministic composition helpers that validate feature package contributions imported by Goddard-owned composition roots.

Feature packages should import IPC route primitives from `@goddard-ai/ipc` and backend route primitives from `@goddard-ai/backend-plugin`. This package references those route trees as daemon plugin metadata.

Daemon plugins may return a named `provides` map from `setup()` and list other daemon plugin definitions in `consumes`. The `setup(context)` callback receives the consumed plugins' provided feature extensions as first-class context fields, such as `context.session`, while daemon-owned substrate remains the responsibility of the daemon composition root.

Daemon plugins may also contribute a namespaced JSON config fragment. The feature owns the schema and meaning of that fragment, but the daemon/core config substrate still owns file discovery, persistence, merge precedence, validation errors, hot reload, and the root config files such as `~/.goddard/config.json` and project-level `.goddard/config.json`.

Daemon plugins that call backend APIs declare `backendRoutes`. During setup, `context.backend` exposes only the route namespaces declared by that plugin, declared by consumed plugins, or contributed by daemon-owned substrate.

## Contract Shape

Use `definePlugin()` from `features/<name>/src/daemon.ts` to preserve exact plugin metadata for static composition:

```ts
export const inboxPlugin = definePlugin({
  name: "inbox",
  ipcRoutes: inboxIpcRoutes,
  setup({ inbox }) {
    return {
      ipcHandlers: {
        inbox: {
          list: ({ body }) => inbox.list(body),
        },
      },
    }
  },
})
```

Feature interop is declared through package imports instead of string names:

```ts
export const inboxPlugin = definePlugin({
  name: "inbox",
  consumes: [sessionPlugin],
  setup(context) {
    return {
      provides: {
        inbox: {
          startWatchingSessionTurns: () => context.session.turnEnded.subscribe(handleTurnEnded),
        },
      },
    }
  },
})
```

The `provides` map returned by `setup()` is the feature-owned extension surface consumed by other feature packages. It can contain methods, event channels, or other typed feature capabilities, but it should not expose feature-private implementation details such as manager instances. `setup(context)` parameters are inferred from daemon substrate plus the plugin's declared `consumes`, `db`, `backendRoutes`, `config`, and `ipcRoutes`.

Composition roots use `composePlugins()` after statically importing the feature daemon entrypoints. Composition validates that feature names are unique, every consumed feature is present, feature dependencies are acyclic, route fragments do not collide, and config fragments are grouped under the contributing feature name.

```ts
const daemonFeatures = composePlugins([sessionPlugin, inboxPlugin])
```
