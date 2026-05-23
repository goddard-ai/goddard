# `@goddard-ai/sdk-plugin`

Internal SDK plugin support contracts for feature packages.

This package is infrastructure for statically composed Goddard feature packages. It is not a public plugin platform and should stay close to type-only until feature SDK composition needs runtime helpers.

## Contract Shape

Feature packages export SDK plugins from `features/<name>/src/sdk.ts` and public SDK composition roots import those plugins:

```ts
export const inboxSdkPlugin = defineSdkPlugin({
  name: "inbox",
  ipcRoutes: inboxIpcRoutes,
  extend({ client }) {
    return {
      inbox: {
        list: (input = {}) => client.inbox.list({ body: input }),
      },
    }
  },
})
```

Feature SDK plugins should not import `@goddard-ai/sdk`. They declare the IPC route tree they wrap and receive a route-scoped client from the public SDK composition root. The optional `extend()` hook is for product-shaped wrappers around generated route methods; route-only plugins can omit it.

`features/inbox/src/sdk.ts` is the reference SDK feature entrypoint.
