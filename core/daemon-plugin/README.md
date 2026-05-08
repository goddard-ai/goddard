# `@goddard-ai/daemon-plugin`

Internal daemon plugin support contracts for feature packages.

This package is infrastructure for statically composed Goddard feature packages. It is not a public plugin platform and should stay close to type-only until daemon feature composition needs runtime helpers.

Feature packages should import IPC schema primitives and composition helpers from `@goddard-ai/ipc`. This package only references IPC schemas as daemon plugin metadata.

Daemon plugins may expose a named `provides` map and list other daemon plugin definitions in `consumes`. The `setup(context)` callback receives the consumed plugins' provided feature extensions as first-class context fields, such as `context.session`, while daemon-owned substrate remains the responsibility of the daemon composition root.

## Contract Shape

Use `defineDaemonPlugin()` from `features/<name>/src/daemon.ts` to preserve exact plugin metadata for static composition:

```ts
export const inboxDaemonPlugin = defineDaemonPlugin({
  name: "inbox",
  ipc: inboxIpcSchema,
  createRequestHandlers: createInboxRequestHandlers,
})
```

Feature interop is declared through package imports instead of string names:

```ts
export const inboxDaemonPlugin = defineDaemonPlugin({
  name: "inbox",
  consumes: [sessionDaemonPlugin],
  setup(context) {
    context.session.turnEnded.subscribe(handleTurnEnded)
  },
})
```

The `provides` map is the only feature-owned extension surface. It can contain methods, event channels, or other typed feature capabilities, but it should not expose feature-private implementation details such as manager instances.
