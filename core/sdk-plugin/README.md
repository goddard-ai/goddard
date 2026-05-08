# `@goddard-ai/sdk-plugin`

Internal SDK plugin support contracts for feature packages.

This package is infrastructure for statically composed Goddard feature packages. It is not a public plugin platform and should stay close to type-only until feature SDK composition needs runtime helpers.

## Contract Shape

Feature packages export SDK plugins from `features/<name>/src/sdk.ts` and public SDK composition roots import those plugins:

```ts
export const inboxSdkPlugin = defineSdkPlugin({
  name: "inbox",
  namespace: "inbox",
  create({ client }) {
    return createInboxNamespace(client)
  },
})
```

Feature SDK plugins should not import `@goddard-ai/sdk`. They receive layer-owned dependencies, such as the daemon IPC client, from the public SDK composition root.

`features/inbox/src/sdk.ts` is the reference SDK feature entrypoint.
