# `@goddard-ai/app-plugin`

Internal app plugin support contracts for feature packages.

This package is infrastructure for statically composed Goddard feature packages. It is not a public plugin platform, remains SDK-agnostic, and should stay close to type-only until app feature composition needs runtime helpers.

## Contract Shape

Feature packages export app plugins from `features/<name>/src/app.tsx`:

```ts
export const inboxAppPlugin = defineAppPlugin({
  name: "inbox",
  sdk: {} as InboxAppSdkRequirements,
  navigation: {
    slot: "primaryWorkbench",
    id: "inbox",
    label: "Inbox",
    icon: "tabs/inbox",
  },
})
```

The app plugin support package must not import or know about `@goddard-ai/sdk`. App feature entrypoints can express SDK requirements at the type level, and the static app composition root owns the actual SDK instance, shell placement, command routing, and shortcut conflict semantics.

`features/inbox/src/app.tsx` is the reference app feature entrypoint.
