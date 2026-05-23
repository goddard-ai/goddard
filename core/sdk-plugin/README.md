# `@goddard-ai/sdk-plugin`

Internal SDK plugin support contracts for feature packages.

This package is infrastructure for statically composed Goddard feature packages. It is not a public plugin platform and should stay close to type-only until feature SDK composition needs runtime helpers.

## Contract Shape

Feature packages export SDK plugins from `features/<name>/src/sdk.ts` and public SDK composition roots import those plugins:

```ts
export const inboxSdkPlugin = defineSdkPlugin({
  name: "inbox",
  ipcRoutes: inboxIpcRoutes,
  wrap({ client }) {
    return {
      inbox: {
        list: (input = {}) => client.inbox.list({ body: input }),
      },
    }
  },
})
```

Feature SDK plugins should not import `@goddard-ai/sdk`. They declare the IPC route tree they wrap and receive a route-scoped client from the public SDK composition root. The optional `wrap()` hook is for product-shaped wrappers around generated route methods; route-only plugins can omit it.

The public SDK composition root owns the client runtime. It composes feature `ipcRoutes`, builds one generated route client, and passes that client to each feature wrapper. Wrappers may expose friendlier SDK method signatures, combine multiple route calls, normalize subscription lifecycles, or preserve a product-shaped namespace when the raw route shape is too transport-oriented. Duplicate wrapper methods in the same namespace are rejected during composition.

`features/inbox/src/sdk.ts` is the reference SDK feature entrypoint.
